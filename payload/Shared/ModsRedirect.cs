// Mods-folder redirect shared by both payloads: Rwpm.Payload (net48, loaded by Doorstop on
// Unity/Mono) and Rwpm.Payload.Hook (net6 startup hook on Waylith/CoreCLR). Both set the
// same private static. C# 9, no nullable annotations, so net48 can compile it.
#nullable disable
using System;
using System.IO;
using System.Reflection;

namespace Rwpm.Shared
{
    internal static class ModsRedirect
    {
        internal const string ModsDirEnv = "RWPM_MODS_DIR";
        internal const string LogEnv = "RWPM_PAYLOAD_LOG";
        const string TargetType = "Verse.GenFilePaths";
        const string TargetField = "modsFolderPath";

        /// <summary>A preset path skips the game's own folder creation. True if created.</summary>
        internal static bool EnsureDir(string modsDir)
        {
            if (Directory.Exists(modsDir))
            {
                return false;
            }
            Directory.CreateDirectory(modsDir);
            return true;
        }

        /// <summary>True when this assembly defines the game's GenFilePaths.</summary>
        internal static bool HoldsTarget(Assembly asm)
        {
            return !asm.IsDynamic && asm.GetType(TargetType, false) != null;
        }

        /// <summary>
        /// Sets the mods path on this copy of GenFilePaths (each Prepatcher reload is a
        /// new copy). Throws when the field is gone: the caller fails closed.
        /// </summary>
        internal static void Patch(Assembly asm, string modsDir)
        {
            FieldInfo field = asm.GetType(TargetType, true)
                .GetField(TargetField, BindingFlags.Static | BindingFlags.NonPublic);
            if (field == null || field.FieldType != typeof(string))
            {
                throw new MissingFieldException(TargetType, TargetField);
            }
            field.SetValue(null, modsDir);
        }

        internal static string Describe(Assembly asm)
        {
            return asm.GetName().Name + "#" + asm.GetHashCode();
        }

        internal static void Log(string logPath, string line)
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
