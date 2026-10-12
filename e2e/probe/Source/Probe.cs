using System;
using System.IO;
using System.Linq;
using Verse;
using Verse.Steam;

namespace Rwpm.SpikeProbe
{
    /// <summary>
    /// Appends one tab-separated line per game start to $RWPM_SPIKE_DIR/probe.log,
    /// then restarts once (if restart.flag exists) or quits.
    /// </summary>
    [StaticConstructorOnStartup]
    public static class Probe
    {
        static Probe()
        {
            string dir = Environment.GetEnvironmentVariable("RWPM_SPIKE_DIR");
            if (string.IsNullOrEmpty(dir))
            {
                Log.Error("[rwpm probe] RWPM_SPIKE_DIR unset");
                return;
            }

            string[] fields =
            {
                "pid=" + System.Diagnostics.Process.GetCurrentProcess().Id,
                "arg0=" + Environment.GetCommandLineArgs()[0],
                "mods=" + GenFilePaths.ModsFolderPath,
                "save=" + GenFilePaths.SaveDataFolderPath,
                "steam=" + SteamManager.Initialized,
                "gfp=" + typeof(GenFilePaths).Assembly.GetHashCode(),
                "installed=" + string.Join(",", ModLister.AllInstalledMods.Select(m => m.PackageId + "@" + m.RootDir.FullName)),
                "active=" + string.Join(",", ModsConfig.ActiveModsInLoadOrder.Select(m => m.PackageId)),
                Env("DOORSTOP_INITIALIZED"),
                Env("DOORSTOP_DISABLE"),
                Env("LD_PRELOAD"),
                Env("SteamAppId"),
                Env("RWPM_MODS_DIR"),
            };
            File.AppendAllText(Path.Combine(dir, "probe.log"), string.Join("\t", fields) + "\n");

            // ?U2: what the game's "Open folder" buttons do (Dialog_Options.cs:292-301).
            string openUrl = Path.Combine(dir, "openurl.flag");
            if (File.Exists(openUrl))
            {
                File.Delete(openUrl);
                UnityEngine.Application.OpenURL(dir);
            }

            string flag = Path.Combine(dir, "restart.flag");
            if (File.Exists(flag))
            {
                File.Delete(flag);
                LongEventHandler.ExecuteWhenFinished(GenCommandLine.Restart);
            }
            else
            {
                LongEventHandler.ExecuteWhenFinished(Root.Shutdown);
            }
        }

        static string Env(string name)
        {
            return name + "=" + (Environment.GetEnvironmentVariable(name) ?? "<unset>");
        }
    }
}
