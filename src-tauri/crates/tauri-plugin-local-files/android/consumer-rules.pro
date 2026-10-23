# Proguard rules for consumers of this library.
#
# The host app builds release with `isMinifyEnabled = true`. Tauri finds the
# plugin class by name through reflection and populates `@InvokeArg` data
# classes by field name, so R8 renaming either one makes the plugin fail
# *only in release* — debug keeps working, which is the hardest kind of
# failure to reproduce.
-keep @app.tauri.annotation.TauriPlugin class * { *; }
-keep class com.gbclstudio.gmplayer.localfiles.** { *; }
-keepclassmembers class com.gbclstudio.gmplayer.localfiles.** {
    <fields>;
    <init>(...);
}
