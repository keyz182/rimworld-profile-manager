// T63 spike: does RimWorld 1.7 (Waylith, CoreCLR) run DOTNET_STARTUP_HOOKS early enough to
// redirect the mods folder? Same redirect as the Doorstop payload: set the private static
// Verse.GenFilePaths.modsFolderPath when Assembly-CSharp loads (any load context).
// Channel: RWPM_MODS_DIR (absent -> no-op), RWPM_PAYLOAD_LOG (log file).
using System;
using System.Collections;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Threading;

internal static class StartupHook
{
    private const string ModsDirEnv = "RWPM_MODS_DIR";
    private const string LogEnv = "RWPM_PAYLOAD_LOG";
    private const string GameEntryAssembly = "RimWorld";

    private static string? _modsDir;

    public static void Initialize()
    {
        string entry = Assembly.GetEntryAssembly()?.GetName().Name ?? "<none>";
        Log($"start pid={Environment.ProcessId} path={Environment.ProcessPath} entry={entry}");
        if (entry != GameEntryAssembly)
        {
            Log("not RimWorld; no-op");
            return;
        }
        _modsDir = Environment.GetEnvironmentVariable(ModsDirEnv);
        if (string.IsNullOrEmpty(_modsDir))
        {
            Log($"no {ModsDirEnv}; no-op");
            return;
        }
        RedirectLog();
        AppDomain.CurrentDomain.AssemblyLoad += (_, e) => TryPatch(e.LoadedAssembly);
        foreach (Assembly asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            TryPatch(asm);
        }
        new Thread(Verify) { IsBackground = true, Name = "rwpm-spike-verify" }.Start();
        Log("hook armed");
    }

    // Waylith's FileLog takes the first SetDirectory call and ignores later ones, so calling
    // it before Main puts Logs/Player.log under the profile instead of the default save dir.
    private static void RedirectLog()
    {
        string? dir = Environment.GetEnvironmentVariable("RWPM_LOG_DIR");
        if (string.IsNullOrEmpty(dir))
        {
            return;
        }
        try
        {
            Type.GetType("Waylith.Logging.FileLog, Waylith", true)!
                .GetMethod("SetDirectory", BindingFlags.Public | BindingFlags.Static)!
                .Invoke(null, new object[] { dir });
            Log($"log directory set -> {dir}");
        }
        catch (Exception ex)
        {
            Log($"log redirect failed: {ex}");
        }
    }

    private static void TryPatch(Assembly asm)
    {
        if (asm.GetName().Name != "Assembly-CSharp")
        {
            return;
        }
        try
        {
            FieldInfo field = asm.GetType("Verse.GenFilePaths", true)!
                .GetField("modsFolderPath", BindingFlags.Static | BindingFlags.NonPublic)!;
            field.SetValue(null, _modsDir);
            Log($"patched Assembly-CSharp#{asm.GetHashCode()} -> {_modsDir}");
        }
        catch (Exception ex)
        {
            Log($"patch failed: {ex}");
        }
    }

    // Reads the game's own view back once ModLister has run: proof, not assumption.
    private static void Verify()
    {
        for (int i = 0; i < 24; i++)
        {
            Thread.Sleep(10000);
            try
            {
                foreach (Assembly asm in AppDomain.CurrentDomain.GetAssemblies())
                {
                    if (asm.GetName().Name != "Assembly-CSharp")
                    {
                        continue;
                    }
                    string folder = (string)asm.GetType("Verse.GenFilePaths", true)!
                        .GetProperty("ModsFolderPath")!.GetValue(null)!;
                    var mods = (IEnumerable)asm.GetType("Verse.ModLister", true)!
                        .GetProperty("AllInstalledMods")!.GetValue(null)!;
                    int n = 0;
                    foreach (object m in mods)
                    {
                        Type t = m.GetType();
                        object root = t.GetProperty("RootDir")!.GetValue(m)!;
                        string pkg = (string)t.GetProperty("PackageId")!.GetValue(m)!;
                        string dir = root.ToString()!;
                        if (dir.StartsWith(folder, StringComparison.Ordinal) || pkg.Contains("rwpm"))
                        {
                            Log($"verify local mod {pkg} @ {dir}");
                        }
                        n++;
                    }
                    if (n > 0)
                    {
                        Log($"verify ModsFolderPath={folder} installed={n}");
                        return;
                    }
                }
            }
            catch (Exception ex)
            {
                Log($"verify not ready: {ex.GetType().Name}: {ex.Message}");
            }
        }
        Log("verify gave up");
    }

    private static void Log(string line)
    {
        string? path = Environment.GetEnvironmentVariable(LogEnv);
        if (string.IsNullOrEmpty(path))
        {
            return;
        }
        try
        {
            File.AppendAllText(path, $"{DateTime.UtcNow:O} [{Environment.ProcessId}] {line}\n");
        }
        catch (IOException)
        {
            // Logging must never break the host process.
        }
    }
}
