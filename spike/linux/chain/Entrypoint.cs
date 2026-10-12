using System;
using System.IO;

namespace Doorstop
{
    /// <summary>Writes $RWPM_SPIKE_DIR/chain.log when our payload chain-loads it.</summary>
    public static class Entrypoint
    {
        public static void Start()
        {
            string dir = Environment.GetEnvironmentVariable("RWPM_SPIKE_DIR");
            if (!string.IsNullOrEmpty(dir))
            {
                File.AppendAllText(Path.Combine(dir, "chain.log"), "chain ran pid="
                    + System.Diagnostics.Process.GetCurrentProcess().Id + Environment.NewLine);
            }
        }
    }
}
