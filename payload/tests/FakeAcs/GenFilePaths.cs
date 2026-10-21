using System;
using System.IO;

namespace Verse
{
    // Same shape as the game: a private static, null until first read, filled in by
    // the getter. No initialiser and no static constructor, so a value set before first
    // use is the one the game sees.
    public static class GenFilePaths
    {
#if NO_FIELD
        public static string ModsFolderPath => Path.Combine(AppContext.BaseDirectory, "Mods");
#else
        private static string modsFolderPath;

        public static string ModsFolderPath
        {
            get
            {
                if (modsFolderPath == null)
                {
                    modsFolderPath = Path.Combine(AppContext.BaseDirectory, "Mods");
                }
                return modsFolderPath;
            }
        }
#endif
    }
}
