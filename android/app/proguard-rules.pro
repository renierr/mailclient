# Called only from Rust over JNI (crates/mailffi/src/android.rs), by name
# and signature: R8 sees no Java caller and would rename or drop them.
-keep interface de.renier.mailclient.PushCallbacks { *; }
-keepclassmembers class * implements de.renier.mailclient.PushCallbacks {
    void onBusy(boolean);
    void onReport(java.lang.String);
}
# Same for the job-event listener: without this R8 drops onJobEvent and
# every queued/finished event dies in deliver_job_json, leaving the busy
# indicator and the status strip permanently silent on release builds.
-keep interface de.renier.mailclient.JobCallbacks { *; }
-keepclassmembers class * implements de.renier.mailclient.JobCallbacks {
    void onJobEvent(java.lang.String);
}
-keepclasseswithmembernames class de.renier.mailclient.MailNative {
    native <methods>;
}
# Referenced from the manifest and started by explicit intent, so keep whole.
-keep class de.renier.mailclient.MainActivity { *; }
