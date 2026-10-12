using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;

namespace Doorstop
{
    /// <summary>
    /// UnityDoorstop entry. Runs at Mono JIT init, before any game assembly loads.
    /// Redirects RimWorld's mods folder to the profile's Mods dir (OFFLINE profiles).
    /// Fails closed: any error ends the process before the game starts.
    /// </summary>
    public static class Entrypoint
    {
        const string ModsDirEnv = "RWPM_MODS_DIR";
        const string LogEnv = "RWPM_PAYLOAD_LOG";
        const string TargetType = "Verse.GenFilePaths";
        const string TargetField = "modsFolderPath";
        const string EnabledEnv = "DOORSTOP_ENABLED";
        const string RestartEnv = "RWPM_RESTART";
        const string ChainEnv = "RWPM_CHAIN_TARGET";

        static string modsDir;
        static string logPath;
#if RWPM_SPIKE
        static bool patchedOnce;
#endif

        public static void Start()
        {
            try
            {
                logPath = Environment.GetEnvironmentVariable(LogEnv);
                modsDir = Environment.GetEnvironmentVariable(ModsDirEnv);
                Log("start pid=" + System.Diagnostics.Process.GetCurrentProcess().Id
                    + " argv=" + string.Join(" ", Environment.GetCommandLineArgs()));

                if (string.IsNullOrEmpty(modsDir))
                {
                    Log("no " + ModsDirEnv + "; nothing to do");
                    return;
                }

                ClearDoorstopGuards();
                EnsureModsDir();

                AppDomain.CurrentDomain.AssemblyLoad += OnAssemblyLoad;
                foreach (Assembly asm in AppDomain.CurrentDomain.GetAssemblies())
                {
                    TryPatch(asm);
                }
                DisableDoorstopForHelpers();
            }
            catch (Exception ex)
            {
                Fail("start failed", ex);
            }
            // Outside the fail-closed block: the redirect is already in place.
            ChainLoad();
        }

        /// <summary>
        /// T12: our launch arguments replace a reused foreign Doorstop's target assembly
        /// (e.g. pardeike's Doorstop.dll), so start that one too. A failure only loses
        /// the other tool's features; the mods redirect stays, so log and carry on.
        /// </summary>
        static void ChainLoad()
        {
            string target = Environment.GetEnvironmentVariable(ChainEnv);
            if (string.IsNullOrEmpty(target))
            {
                return;
            }
            try
            {
                Assembly asm = Assembly.LoadFrom(target);
                MethodInfo start = asm.GetType("Doorstop.Entrypoint", true)
                    .GetMethod("Start", BindingFlags.Public | BindingFlags.Static);
                if (start == null)
                {
                    throw new MissingMethodException("Doorstop.Entrypoint", "Start");
                }
                start.Invoke(null, null);
                Log("chain-loaded " + target);
            }
            catch (Exception ex)
            {
                Log("chain-load failed for " + target + ": " + ex);
            }
        }

        /// <summary>V1: let an in-game restart child re-run Doorstop.</summary>
        static void ClearDoorstopGuards()
        {
#if RWPM_SPIKE
            if (Environment.GetEnvironmentVariable("RWPM_SPIKE_NO_CLEAR") == "1")
            {
                Log("spike: NO_CLEAR set, guards kept");
                return;
            }
#endif
            Environment.SetEnvironmentVariable("DOORSTOP_INITIALIZED", null);
            Environment.SetEnvironmentVariable("DOORSTOP_DISABLE", null);
            Log("cleared DOORSTOP_INITIALIZED, DOORSTOP_DISABLE");
        }

        /// <summary>
        /// V28 (Linux): with DOORSTOP_ENABLED set, Doorstop hooks dup2 in every process the
        /// game starts (xdg-open for "Open folder"), breaking their shell redirects. Only when
        /// rwpm's restart trampoline is in place (RWPM_RESTART) is it safe to drop: the
        /// in-game Restart then re-enters through rwpm, which sets it again (V1).
        /// </summary>
        static void DisableDoorstopForHelpers()
        {
            if (Environment.OSVersion.Platform != PlatformID.Unix
                || string.IsNullOrEmpty(Environment.GetEnvironmentVariable(RestartEnv)))
            {
                return;
            }
#if RWPM_SPIKE
            if (Environment.GetEnvironmentVariable("RWPM_SPIKE_KEEP_ENABLED") == "1")
            {
                Log("spike: KEEP_ENABLED set, " + EnabledEnv + " kept");
                return;
            }
#endif
            Environment.SetEnvironmentVariable(EnabledEnv, null);
            try
            {
                // Children inherit the native environ; make sure it changed too.
                unsetenv(EnabledEnv);
            }
            catch (Exception ex)
            {
                Log("libc unsetenv unavailable: " + ex.Message);
            }
            Log("unset " + EnabledEnv + " for game-started helpers");
        }

        [DllImport("libc", EntryPoint = "unsetenv")]
        static extern int unsetenv(string name);

        /// <summary>V16: a preset path skips the game's own folder creation.</summary>
        static void EnsureModsDir()
        {
#if RWPM_SPIKE
            if (Environment.GetEnvironmentVariable("RWPM_SPIKE_NO_CREATE") == "1")
            {
                Log("spike: NO_CREATE set, dir not created");
                return;
            }
#endif
            if (!Directory.Exists(modsDir))
            {
                Directory.CreateDirectory(modsDir);
                Log("created " + modsDir);
            }
        }

        static void OnAssemblyLoad(object sender, AssemblyLoadEventArgs args)
        {
            try
            {
                TryPatch(args.LoadedAssembly);
            }
            catch (Exception ex)
            {
                Fail("patch failed for " + args.LoadedAssembly.FullName, ex);
            }
        }

        /// <summary>V2: patch every copy of GenFilePaths, incl. Prepatcher's in-process reload.</summary>
        static void TryPatch(Assembly asm)
        {
            if (asm.IsDynamic)
            {
                return;
            }
            Type type = asm.GetType(TargetType, false);
            if (type == null)
            {
                return;
            }
#if RWPM_SPIKE
            if (patchedOnce && Environment.GetEnvironmentVariable("RWPM_SPIKE_NO_RELOAD_HOOK") == "1")
            {
                Log("spike: NO_RELOAD_HOOK set, skipped " + Describe(asm));
                return;
            }
#endif
            FieldInfo field = type.GetField(TargetField, BindingFlags.Static | BindingFlags.NonPublic);
            if (field == null || field.FieldType != typeof(string))
            {
                throw new MissingFieldException(TargetType, TargetField);
            }
            field.SetValue(null, modsDir);
#if RWPM_SPIKE
            patchedOnce = true;
#endif
            Log("patched " + Describe(asm) + " -> " + modsDir);
        }

        static string Describe(Assembly asm)
        {
            return asm.GetName().Name + "#" + asm.GetHashCode();
        }

        static void Fail(string what, Exception ex)
        {
            Log("FATAL " + what + ": " + ex);
            Environment.FailFast("rwpm payload: " + what + ": " + ex.Message);
        }

        static void Log(string line)
        {
            if (string.IsNullOrEmpty(logPath))
            {
                return;
            }
            try
            {
                File.AppendAllText(logPath, DateTime.UtcNow.ToString("o") + " " + line + Environment.NewLine);
            }
            catch (IOException)
            {
                // Logging must never break the redirect itself.
            }
        }
    }
}
