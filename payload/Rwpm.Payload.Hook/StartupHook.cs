using System;
using System.IO;
using System.Reflection;
using Rwpm.Shared;

/// <summary>
/// Startup hook for RimWorld 1.7 (Waylith, CoreCLR). The runtime calls Initialize before the
/// game's Main (R39). OFFLINE profiles: redirects the mods folder to RWPM_MODS_DIR (V64) and
/// the game log to the profile folder (V67). The runtime looks the type up by this exact
/// name, so it stays in the global namespace.
/// </summary>
internal static class StartupHook
{
    const string GameEntryAssembly = "RimWorld";
    const string FileLogType = "Waylith.Logging.FileLog, Waylith";

    static string? modsDir;
    static string? logPath;

    public static void Initialize()
    {
        logPath = Environment.GetEnvironmentVariable(ModsRedirect.LogEnv);
        string entry = Assembly.GetEntryAssembly()?.GetName().Name ?? "<none>";
        Log("hook start pid=" + Environment.ProcessId + " entry=" + entry);

        // V65: every .NET process the game starts inherits the env; only the game is ours.
        if (entry != GameEntryAssembly)
        {
            Log("not RimWorld; nothing to do");
            return;
        }
        modsDir = Environment.GetEnvironmentVariable(ModsRedirect.ModsDirEnv);
        if (string.IsNullOrEmpty(modsDir))
        {
            Log("no " + ModsRedirect.ModsDirEnv + "; nothing to do");
            return;
        }

        try
        {
            if (ModsRedirect.EnsureDir(modsDir))
            {
                Log("created " + modsDir);
            }
            AppDomain.CurrentDomain.AssemblyLoad += OnAssemblyLoad;
            foreach (Assembly asm in AppDomain.CurrentDomain.GetAssemblies())
            {
                TryPatch(asm);
            }
        }
        catch (Exception ex)
        {
            Fail("start failed", ex);
        }
        // Outside the fail-closed block: the mods redirect is already in place.
        RedirectLog();
    }

    /// <summary>
    /// V67: Waylith's FileLog keeps the first SetDirectory call (R41). Calling it before Main
    /// puts Logs/Player.log in the profile folder (the parent of its Mods folder) instead of
    /// the Default save dir. A failure here leaves mods isolation intact, so it only logs.
    /// </summary>
    static void RedirectLog()
    {
        try
        {
            string? profile = Path.GetDirectoryName(
                Path.TrimEndingDirectorySeparator(Path.GetFullPath(modsDir!)));
            if (string.IsNullOrEmpty(profile))
            {
                Log("log redirect skipped: no parent folder for " + modsDir);
                return;
            }
            Type.GetType(FileLogType, true)!
                .GetMethod("SetDirectory", BindingFlags.Public | BindingFlags.Static, null,
                    new[] { typeof(string) }, null)!
                .Invoke(null, new object[] { profile });
            Log("log directory -> " + profile);
        }
        catch (Exception ex)
        {
            Log("log redirect failed: " + ex);
        }
    }

    static void OnAssemblyLoad(object? sender, AssemblyLoadEventArgs args)
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

    static void TryPatch(Assembly asm)
    {
        if (!ModsRedirect.HoldsTarget(asm))
        {
            return;
        }
        ModsRedirect.Patch(asm, modsDir!);
        Log("patched " + ModsRedirect.Describe(asm) + " -> " + modsDir);
    }

    /// <summary>V26: the game must not start on the install's Mods folder.</summary>
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
