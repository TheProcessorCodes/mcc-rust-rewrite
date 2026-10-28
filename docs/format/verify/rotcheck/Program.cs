// Empirical probe of the rotation convention Reclaimer uses for scenario placements.
// Reclaimer (Reclaimer.Core\Geometry\Vectors\EulerAngles.cs, explicit operator Quaternion) builds
//   q = AxisAngle(+X, roll) * AxisAngle(-Y, pitch) * AxisAngle(+Z, yaw)      (System.Numerics operator *)
// and ObjectPlacement.SetTransform builds  M = Scale(s) * FromQuaternion(q) * Translation(p).
// This program re-expresses that one-line formula and prints what it does to the unit axes, so the
// Rust port can match (or deliberately diverge from) it. Run: dotnet run --project rotcheck.csproj
using System.Numerics;

static Quaternion ReclaimerQuat(float yaw, float pitch, float roll) =>
    Quaternion.CreateFromAxisAngle(Vector3.UnitX, roll)
    * Quaternion.CreateFromAxisAngle(-Vector3.UnitY, pitch)
    * Quaternion.CreateFromAxisAngle(Vector3.UnitZ, yaw);

static string V(Vector3 v) => $"({v.X,7:F3}, {v.Y,7:F3}, {v.Z,7:F3})";

static void Probe(string label, float yawDeg, float pitchDeg, float rollDeg)
{
    float D2R = MathF.PI / 180f;
    float yaw = yawDeg * D2R, pitch = pitchDeg * D2R, roll = rollDeg * D2R;
    var q = ReclaimerQuat(yaw, pitch, roll);
    var m = Matrix4x4.CreateFromQuaternion(q);
    // "Halo angles_to_vector" expectation for the forward (+X) axis: (cos y cos p, sin y cos p, sin p)
    var expectFwd = new Vector3(MathF.Cos(yaw) * MathF.Cos(pitch), MathF.Sin(yaw) * MathF.Cos(pitch), MathF.Sin(pitch));
    Console.WriteLine($"{label}: yaw {yawDeg} pitch {pitchDeg} roll {rollDeg} deg");
    Console.WriteLine($"   +X (forward) -> {V(Vector3.Transform(Vector3.UnitX, q))}   via matrix {V(Vector3.Transform(Vector3.UnitX, m))}   Halo fwd formula {V(expectFwd)}");
    Console.WriteLine($"   +Y (left)    -> {V(Vector3.Transform(Vector3.UnitY, q))}");
    Console.WriteLine($"   +Z (up)      -> {V(Vector3.Transform(Vector3.UnitZ, q))}");
}

// Order-of-multiplication sanity check: does q1*q2 apply q2 first or q1 first?
{
    var rz90 = Quaternion.CreateFromAxisAngle(Vector3.UnitZ, MathF.PI / 2);   // +X -> +Y
    var rx90 = Quaternion.CreateFromAxisAngle(Vector3.UnitX, MathF.PI / 2);   // +Y -> +Z
    var a = rx90 * rz90;   // Reclaimer-style order: AxisAngle(X) * AxisAngle(Z)
    var b = rz90 * rx90;
    Console.WriteLine("System.Numerics quaternion product order check (Rz90 takes +X to +Y, Rx90 takes +Y to +Z):");
    Console.WriteLine($"   (Rx90 * Rz90) applied to +X = {V(Vector3.Transform(Vector3.UnitX, a))}   -> if (0,0,1): Rz applied first, then Rx (right factor first)");
    Console.WriteLine($"   (Rz90 * Rx90) applied to +X = {V(Vector3.Transform(Vector3.UnitX, b))}   -> if (0,1,0): Rx applied first (no effect on +X), then Rz");
    Console.WriteLine($"   Matrix(Rx90*Rz90) == Matrix(Rz90)*Matrix(Rx90)? {Matrix4x4.CreateFromQuaternion(a) == Matrix4x4.CreateFromQuaternion(rz90) * Matrix4x4.CreateFromQuaternion(rx90)} (row-vector matrices compose left-to-right)");
    Console.WriteLine();
}

Probe("A", 90, 30, 0);
Probe("B", 0, 30, 0);
Probe("C", 0, 90, 0);
Probe("D", 0, 0, 90);
Probe("E riverworld scenery[0]", -87.53f, 0, 0);
Probe("F zanzibar dome_light", -180, 90, -180);
Probe("G 020_base sewer_fan2", 0, 0, -90);

// Full placement transform, row-vector convention: world = local * M ; translation lives in row 4 (M41..M43).
{
    var q = ReclaimerQuat(-1.52768f, 0, 0);
    var M = Matrix4x4.CreateScale(1f) * Matrix4x4.CreateFromQuaternion(q) * Matrix4x4.CreateTranslation(new Vector3(79.7632f, -56.7296f, 1.5002f));
    Console.WriteLine();
    Console.WriteLine("riverworld scenery[0] full transform M = Scale(1) * Rot(q) * Translation(pos)  (scale 0 is treated as 1):");
    Console.WriteLine($"   row1 {M.M11,8:F4} {M.M12,8:F4} {M.M13,8:F4} {M.M14,8:F4}");
    Console.WriteLine($"   row2 {M.M21,8:F4} {M.M22,8:F4} {M.M23,8:F4} {M.M24,8:F4}");
    Console.WriteLine($"   row3 {M.M31,8:F4} {M.M32,8:F4} {M.M33,8:F4} {M.M34,8:F4}");
    Console.WriteLine($"   row4 {M.M41,8:F4} {M.M42,8:F4} {M.M43,8:F4} {M.M44,8:F4}");
    Console.WriteLine($"   local origin -> world {V(Vector3.Transform(Vector3.Zero, M))} ; local +X -> world {V(Vector3.Transform(Vector3.UnitX, M))}");
}
