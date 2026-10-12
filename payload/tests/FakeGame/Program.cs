using System;
using System.IO;
using Verse;
using Waylith.Logging;

internal static class Program
{
    private static int Main()
    {
        // The engine sets its log dir (the Default save dir) at startup; a hook call wins.
        FileLog.SetDirectory(Path.Combine(AppContext.BaseDirectory, "default-logs"));
        Console.WriteLine("mods=" + GenFilePaths.ModsFolderPath);
        Console.WriteLine("logdir=" + FileLog.Directory);
        return 0;
    }
}
