namespace Waylith.Logging
{
    // Same rule as the engine (R41): the first SetDirectory call wins, later ones are ignored.
    public static class FileLog
    {
        public static string Directory { get; private set; }

        public static void SetDirectory(string dir)
        {
            if (Directory == null)
            {
                Directory = dir;
            }
        }
    }
}
