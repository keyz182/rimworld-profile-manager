// T63 spike (V65): a stand-in for a .NET helper the game might start (eg rimmcp-proxy, net8).
// Run with DOTNET_STARTUP_HOOKS set: it must start, print, and exit 0 untouched by the hook.
System.Console.WriteLine($"helper ok pid={System.Environment.ProcessId}");
return 0;
