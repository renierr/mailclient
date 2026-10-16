# explainer-video engine — do not edit in a project; `video-kit upgrade` replaces it.
"""Synthesise each scene and record when every word is spoken.

Reads a job file (JSON: engine, voice settings, scenes with text and output paths) and,
per scene, writes the audio plus a JSON list of word timings in seconds.

Engines:
  kokoro  local neural TTS (Kokoro-82M), offline once the model is cached
  qwen3   local Qwen3-TTS 1.7B: a preset speaker, or a clone of a recorded voice; word times
          come from a forced aligner (MMS), since the model reports none
  edge    Microsoft Edge's online neural voices via edge-tts (sends the text to Microsoft)
"""
import json
import re
import sys
import time
import unicodedata
import wave


def kokoro(job):
    import numpy as np
    from kokoro import KPipeline

    settings = job["settings"]
    rate = 24000
    lang = settings["name"][0]  # voice names start with their language: a = US English, b = UK English
    pipeline = KPipeline(lang_code=lang, repo_id="hexgrad/Kokoro-82M")
    for scene in job["scenes"]:
        parts, words, at = [], [], 0.0
        for result in pipeline(scene["text"], voice=settings["name"], speed=settings["speed"]):
            audio = result.audio.numpy()
            for t in result.tokens or []:
                if t.start_ts is not None and t.end_ts is not None and t.text.strip():
                    words.append({"text": t.text, "start": at + t.start_ts, "end": at + t.end_ts})
            parts.append(audio)
            at += len(audio) / rate
        pcm = (np.clip(np.concatenate(parts), -1, 1) * 32767).astype("<i2").tobytes()
        with wave.open(scene["audio"], "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(rate)
            w.writeframes(pcm)
        done(scene, words)


def qwen3(job):
    import logging

    import numpy as np
    import torch

    # qwen-tts imports the sox package, which warns at length that the SoX binary is missing;
    # only the unused 25 Hz tokenizer needs it. A filter, as sox resets its logger's level.
    logging.getLogger("sox").addFilter(lambda record: record.levelno >= logging.ERROR)
    from qwen_tts import Qwen3TTSModel
    from transformers.utils import logging as transformers_logging

    transformers_logging.set_verbosity_error()  # one "Setting pad_token_id" line per scene otherwise

    settings = job["settings"]
    clone = settings.get("clone")
    device = "cuda:0" if torch.cuda.is_available() else "cpu"
    if device == "cpu":
        print("  no CUDA GPU found: Qwen3 runs on the CPU, expect several minutes per scene", flush=True)
    from huggingface_hub import snapshot_download

    started = time.monotonic()
    print("  loading Qwen3 …", flush=True)
    # Load from the local snapshot, not the repo id: for a repo id transformers asks the Hub
    # about the model on every load, which fails offline even with everything cached.
    model = Qwen3TTSModel.from_pretrained(
        snapshot_download(f"Qwen/Qwen3-TTS-12Hz-1.7B-{'Base' if clone else 'CustomVoice'}"),
        device_map=device,
        dtype=torch.bfloat16 if device != "cpu" else torch.float32,
    )
    if clone:
        prompt = model.create_voice_clone_prompt(ref_audio=clone["audio"], ref_text=clone["text"])
    aligner = Aligner(device)
    print(f"  ready in {clock(time.monotonic() - started)}", flush=True)

    # Qwen3 takes minutes per scene, so each line says how far along the run is. The time
    # left scales the pace so far by the characters still to speak.
    scenes = job["scenes"]
    total_chars = sum(len(s["text"]) for s in scenes)
    done_chars, voicing = 0, time.monotonic()
    for i, scene in enumerate(scenes, 1):
        took = time.monotonic()
        torch.manual_seed(settings["seed"])
        if clone:
            wavs, rate = model.generate_voice_clone(
                text=scene["text"], language=settings["language"], voice_clone_prompt=prompt
            )
        else:
            wavs, rate = model.generate_custom_voice(
                text=scene["text"], language=settings["language"], speaker=settings["name"],
                instruct=settings["instruct"] or None,
            )
        audio = np.asarray(wavs[0], dtype=np.float32)
        # The model leaves silence around the speech; the timeline adds its own lead-in and tail.
        loud = np.flatnonzero(np.abs(audio) > 0.02 * np.abs(audio).max())
        if not loud.size:
            raise SystemExit(f"Qwen3 returned silence for scene {scene['id']}: try another voice.seed.")
        pad = int(0.05 * rate)
        audio = audio[max(0, loud[0] - pad): loud[-1] + pad]
        audio *= 0.89 / max(1e-6, float(np.abs(audio).max()))
        words = aligner.words(audio, rate, scene["text"])
        pcm = (np.clip(audio, -1, 1) * 32767).astype("<i2").tobytes()
        with wave.open(scene["audio"], "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(rate)
            w.writeframes(pcm)
        done_chars += len(scene["text"])
        now = time.monotonic()
        left = (now - voicing) / done_chars * (total_chars - done_chars)
        done(scene, words, f"[{i}/{len(scenes)}] ", f", {len(audio) / rate:.0f} s audio in {clock(now - took)}"
             + (f" · ~{clock(left)} left" if i < len(scenes) else ""))


def clock(seconds):
    return f"{int(seconds) // 60}:{int(seconds) % 60:02d}"


class Aligner:
    """Finds when each word of a known text is spoken (torchaudio's MMS forced aligner)."""

    def __init__(self, device):
        import torchaudio

        self.torchaudio = torchaudio
        self.bundle = torchaudio.pipelines.MMS_FA
        self.model = self.bundle.get_model(with_star=False).to(device)
        self.tokenizer = self.bundle.get_tokenizer()
        self.aligner = self.bundle.get_aligner()
        self.device = device

    def words(self, audio, rate, text):
        import torch

        # MMS knows romanised letters only: accents are folded (ä → a) and anything else dropped.
        # A word left without letters (a bare number) gets no time; voice.mjs interpolates it.
        tokens = []
        for token in text.split():
            core = re.sub(r"^\W+|\W+$", "", token)
            folded = unicodedata.normalize("NFKD", core.lower().replace("ß", "ss")).encode("ascii", "ignore").decode()
            letters = re.sub(r"[^a-z']", "", folded).strip("'")
            if letters:
                tokens.append((core, letters))
        if not tokens:
            return []
        sr = self.bundle.sample_rate
        wave16 = self.torchaudio.functional.resample(torch.from_numpy(audio).unsqueeze(0), rate, sr).to(self.device)
        with torch.inference_mode():
            emission, _ = self.model(wave16)
        spans = self.aligner(emission[0], self.tokenizer([t[1] for t in tokens]))
        seconds = wave16.size(1) / emission.size(1) / sr
        return [
            {"text": core, "start": span[0].start * seconds, "end": span[-1].end * seconds}
            for (core, _), span in zip(tokens, spans)
        ]


def edge(job):
    import asyncio

    import edge_tts

    settings = job["settings"]

    async def one(scene):
        communicate = edge_tts.Communicate(
            scene["text"], settings["name"], rate=settings["rate"], pitch=settings["pitch"], boundary="WordBoundary"
        )
        words = []
        with open(scene["audio"], "wb") as audio:
            async for chunk in communicate.stream():
                if chunk["type"] == "audio":
                    audio.write(chunk["data"])
                elif chunk["type"] == "WordBoundary":
                    words.append({
                        "text": chunk["text"],
                        "start": chunk["offset"] / 1e7,
                        "end": (chunk["offset"] + chunk["duration"]) / 1e7,
                    })
        done(scene, words)

    async def all_scenes():
        for scene in job["scenes"]:
            await one(scene)

    asyncio.run(all_scenes())


def done(scene, words, prefix="", suffix=""):
    with open(scene["words"], "w", encoding="utf-8") as out:
        json.dump(words, out, indent=1)
    print(f"  {prefix}{scene['id']}: {len(words)} words{suffix}", flush=True)


with open(sys.argv[1], encoding="utf-8") as f:
    JOB = json.load(f)
{"kokoro": kokoro, "qwen3": qwen3, "edge": edge}[JOB["engine"]](JOB)
