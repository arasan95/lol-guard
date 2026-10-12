using System;
using System.Collections.Generic;
using Microsoft.Diagnostics.Tracing;
using Microsoft.Diagnostics.Tracing.Parsers;

public static class CSwitchAnalyzer
{
    private sealed class Row
    {
        public int Cpu;
        public int Tid;
        public int Pid;
        public string Name;
        public double Ms;
    }

    public static int Main(string[] args)
    {
        if (args.Length < 2)
        {
            Console.WriteLine("usage: Analyze-CSwitch.exe <trace.etl> <targetTid> [topN]");
            return 2;
        }
        string etl = args[0];
        int target = 0;
        if (!int.TryParse(args[1], out target)) { Console.WriteLine("bad tid"); return 2; }
        int topN = 8;
        if (args.Length >= 3) { int.TryParse(args[2], out topN); }
        if (topN < 1) { topN = 8; }

        Dictionary<string, double> ms = new Dictionary<string, double>();
        Dictionary<string, string> nm = new Dictionary<string, string>();
        Dictionary<int, double> cpuBase = new Dictionary<int, double>();
        double minT = double.MaxValue;
        double maxT = double.MinValue;

        using (var source = new ETWTraceEventSource(etl))
        {
            KernelTraceEventParser k = new KernelTraceEventParser(source);
            k.ThreadCSwitch += delegate(Microsoft.Diagnostics.Tracing.Parsers.Kernel.CSwitchTraceData e)
            {
                int cpu = e.ProcessorNumber;
                double now = e.TimeStampRelativeMSec;
                if (now < minT) { minT = now; }
                if (now > maxT) { maxT = now; }
                if (cpuBase.ContainsKey(cpu))
                {
                    double dt = now - cpuBase[cpu];
                    if (dt >= 0 && dt < 60000)
                    {
                        string key = cpu + ":" + e.OldThreadID;
                        double cur = 0;
                        ms.TryGetValue(key, out cur);
                        ms[key] = cur + dt;
                        if (!nm.ContainsKey(key))
                        {
                            string pname = e.OldProcessName;
                            if (string.IsNullOrEmpty(pname)) { pname = "pid" + e.OldProcessID; }
                            if (e.OldThreadID == 0) { pname = "Idle"; }
                            nm[key] = e.OldProcessID + "|" + pname;
                        }
                    }
                }
                cpuBase[cpu] = now;
            };
            source.Process();
        }

        double window = (maxT > minT) ? (maxT - minT) : 0;
        Console.WriteLine(string.Format("WINDOW ms={0:F0}", window));

        List<Row> rows = new List<Row>();
        foreach (KeyValuePair<string, double> kv in ms)
        {
            string[] p = kv.Key.Split(':');
            int cpu = int.Parse(p[0]);
            int tid = int.Parse(p[1]);
            string[] q = nm[kv.Key].Split('|');
            Row r = new Row();
            r.Cpu = cpu; r.Tid = tid; r.Pid = int.Parse(q[0]); r.Name = q[1]; r.Ms = kv.Value;
            rows.Add(r);
        }

        double targetTotal = 0;
        List<Row> tr = new List<Row>();
        foreach (Row r in rows) { if (r.Tid == target) { tr.Add(r); targetTotal += r.Ms; } }
        tr.Sort(delegate(Row a, Row b) { return b.Ms.CompareTo(a.Ms); });
        Console.WriteLine(string.Format("TARGET tid={0} totalMs={1:F0}", target, targetTotal));
        foreach (Row r in tr)
        {
            double pct = (targetTotal > 0) ? (r.Ms * 100.0 / targetTotal) : 0;
            Console.WriteLine(string.Format("TCPU cpu={0} ms={1:F0} pct={2:F1}", r.Cpu, r.Ms, pct));
        }
        if (tr.Count == 0) { Console.WriteLine("TCPU none (tid not observed)"); return 0; }
        int topCpu = tr[0].Cpu;

        List<Row> cr = new List<Row>();
        double cpuTotal = 0;
        foreach (Row r in rows) { if (r.Cpu == topCpu) { cr.Add(r); cpuTotal += r.Ms; } }
        cr.Sort(delegate(Row a, Row b) { return b.Ms.CompareTo(a.Ms); });
        Console.WriteLine(string.Format("TOPCPU cpu={0} totalMs={1:F0}", topCpu, cpuTotal));
        int n = 0;
        foreach (Row r in cr)
        {
            if (n >= topN + 1) { break; }
            n++;
            double pct = (cpuTotal > 0) ? (r.Ms * 100.0 / cpuTotal) : 0;
            string self = (r.Tid == target) ? " <=TARGET" : "";
            Console.WriteLine(string.Format("TTOP cpu={0} tid={1} pid={2} name={3} ms={4:F0} pct={5:F1}{6}", r.Cpu, r.Tid, r.Pid, r.Name, r.Ms, pct, self));
        }
        return 0;
    }
}
