# explainer-video engine — do not edit in a project; `video-kit upgrade` replaces it.
"""Synthesise each scene and record when every word is spoken.

Reads a job file (JSON: engine, voice settings, scenes with text and output paths) and,
per scene, writes the audio plus a JSON list of word timings in seconds.

Engines:
  kokoro  local neural TTS (Kokoro-82M), offline once the model is cached
  edge    Microsoft Edge's online neural voices via edge-tts (sends the text to Microsoft)
"""
import json
import sys
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


def done(scene, words):
    with open(scene["words"], "w", encoding="utf-8") as out:
        json.dump(words, out, indent=1)
    print(f"  {scene['id']}: {len(words)} words", flush=True)


with open(sys.argv[1], encoding="utf-8") as f:
    JOB = json.load(f)
{"kokoro": kokoro, "edge": edge}[JOB["engine"]](JOB)
