using System;
using System.Reflection;
using System.Runtime.InteropServices;
using Rwpm.Shared;

namespace Doorstop
{
    /// <summary>
    /// UnityDoorstop entry. Runs at Mono JIT init, before any game assembly loads.
    /// Redirects RimWorld's mods folder to the profile's Mods dir (OFFLINE profiles).
    /// Fails closed: any error ends the process before the game starts.
    /// </summary>
    public static class Entrypoint
    {
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
                logPath = Environment.GetEnvironmentVariable(ModsRedirect.LogEnv);
                modsDir = Environment.GetEnvironmentVariable(ModsRedirect.ModsDirEnv);
                Log("start pid=" + System.Diagnostics.Process.GetCurrentProcess().Id
                    + " argv=" + string.Join(" ", Environment.GetCommandLineArgs()));

                if (string.IsNullOrEmpty(modsDir))
                {
                    Log("no " + ModsRedirect.ModsDirEnv + "; nothing to do");
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
        /// Our launch arguments replace a reused foreign Doorstop's target assembly
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

        /// <summary>Let an in-game restart child re-run Doorstop.</summary>
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
        /// Linux: with DOORSTOP_ENABLED set, Doorstop hooks dup2 in every process the
        /// game starts (xdg-open for "Open folder"), breaking their shell redirects. Only when
        /// rwpm's restart trampoline is in place (RWPM_RESTART) is it safe to drop: the
        /// in-game Restart then re-enters through rwpm, which sets it again.
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

        /// <summary>A preset path skips the game's own folder creation.</summary>
        static void EnsureModsDir()
        {
#if RWPM_SPIKE
            if (Environment.GetEnvironmentVariable("RWPM_SPIKE_NO_CREATE") == "1")
            {
                Log("spike: NO_CREATE set, dir not created");
                return;
            }
#endif
            if (ModsRedirect.EnsureDir(modsDir))
            {
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

        /// <summary>Patch every copy of GenFilePaths, incl. Prepatcher's in-process reload.</summary>
        static void TryPatch(Assembly asm)
        {
            if (!ModsRedirect.HoldsTarget(asm))
            {
                return;
            }
#if RWPM_SPIKE
            if (patchedOnce && Environment.GetEnvironmentVariable("RWPM_SPIKE_NO_RELOAD_HOOK") == "1")
            {
                Log("spike: NO_RELOAD_HOOK set, skipped " + ModsRedirect.Describe(asm));
                return;
            }
#endif
            ModsRedirect.Patch(asm, modsDir);
#if RWPM_SPIKE
            patchedOnce = true;
#endif
            Log("patched " + ModsRedirect.Describe(asm) + " -> " + modsDir);
        }

        static void Fail(string what, Exception ex)
        {
            Log("FATAL " + what + ": " + ex);
            Environment.FailFast("rwpm payload: " + what + ": " + ex.Message);
        }

        static void Log(string line)
        {
            ModsRedirect.Log(logPath, line);
        }
    }
}
