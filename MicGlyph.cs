using System.Drawing;
using System.Drawing.Drawing2D;

namespace MicTray;

/// <summary>
/// 共用麦克风绘制：用基础图元（圆角胶囊麦头 + U 形支架 + 底座）矢量绘制，
/// 不依赖任何字体、也不解析 SVG，保证在所有 DPI / 尺寸下都清晰且严格居中。
/// 托盘(32px)与悬浮窗(任意尺寸)共用同一套几何，跨尺寸一致。
/// </summary>
internal static class MicGlyph
{
    public static void Draw(Graphics g, Rectangle r, Color color, bool muted)
    {
        g.SmoothingMode = SmoothingMode.AntiAlias;
        g.PixelOffsetMode = PixelOffsetMode.HighQuality;

        float cx = r.X + r.Width / 2f;
        // 严格几何居中；视觉重心问题后续根据截图再微调
        float cy = r.Y + r.Height / 2f;
        float s = Math.Min(r.Width, r.Height) * 0.84f;

        float cw = s * 0.46f;            // 麦头宽度
        float ch = s * 0.50f;            // 麦头高度
        float arcR = cw * 0.60f;         // U 形支架半径
        float glyphH = ch + 2 * arcR;    // 整体高度（麦头 + 双臂 + 弧底）
        float capTop = cy - glyphH / 2f; // 让整体垂直居中
        float capBottom = capTop + ch;
        float arcCy = capBottom + arcR;  // 弧心 y

        // 1) 麦头（圆角胶囊，做成话筒头造型）
        using (var brush = new SolidBrush(color))
        {
            FillRoundedRect(g, brush, cx - cw / 2f, capTop, cw, ch, cw / 2f);

            // 网罩细节：仅在大尺寸（悬浮窗）绘制，托盘 32px 太细会糊
            if (r.Width >= 64)
            {
                using var net = new Pen(Color.FromArgb(80, Color.Black), Math.Max(1f, s * 0.03f));
                float gTop = capTop + ch * 0.20f;
                float gBot = capBottom - ch * 0.14f;
                for (float y = gTop; y < gBot; y += ch * 0.17f)
                    g.DrawLine(net, cx - cw / 2f + cw * 0.14f, y, cx + cw / 2f - cw * 0.14f, y);
            }
        }

        // 2) U 形支架（两侧竖臂 + 下半圆弧）+ 底座横线
        using (var pen = new Pen(color, Math.Max(2f, s * 0.14f))
        {
            StartCap = LineCap.Round,
            EndCap = LineCap.Round,
            LineJoin = LineJoin.Round
        })
        {
            // 两侧竖臂：从麦头底两角向下接到弧端点
            g.DrawLine(pen, cx - cw / 2f, capBottom, cx - arcR, arcCy);
            g.DrawLine(pen, cx + cw / 2f, capBottom, cx + arcR, arcCy);
            // 下半圆弧（开口朝上）
            g.DrawArc(pen, cx - arcR, arcCy - arcR, arcR * 2f, arcR * 2f, 0f, 180f);
            // 底座横线
            float baseY = arcCy + arcR;
            float baseW = cw * 1.7f;
            g.DrawLine(pen, cx - baseW / 2f, baseY, cx + baseW / 2f, baseY);
        }

        if (muted) DrawSlash(g, r);
    }

    private static void FillRoundedRect(Graphics g, Brush brush, float x, float y, float w, float h, float radius)
    {
        radius = Math.Min(radius, w / 2f);
        radius = Math.Min(radius, h / 2f);
        using var path = new GraphicsPath();
        path.AddArc(x, y, 2 * radius, 2 * radius, 180, 90);
        path.AddArc(x + w - 2 * radius, y, 2 * radius, 2 * radius, 270, 90);
        path.AddArc(x + w - 2 * radius, y + h - 2 * radius, 2 * radius, 2 * radius, 0, 90);
        path.AddArc(x, y + h - 2 * radius, 2 * radius, 2 * radius, 90, 90);
        path.CloseFigure();
        g.FillPath(brush, path);
    }

    private static void DrawSlash(Graphics g, Rectangle r)
    {
        int m = (int)(Math.Min(r.Width, r.Height) * 0.18f);
        using var white = new Pen(Color.White, Math.Max(2f, Math.Min(r.Width, r.Height) * 0.14f))
        {
            StartCap = LineCap.Round, EndCap = LineCap.Round
        };
        using var red = new Pen(Color.FromArgb(0xB0, 0, 0), Math.Max(1f, Math.Min(r.Width, r.Height) * 0.07f))
        {
            StartCap = LineCap.Round, EndCap = LineCap.Round
        };
        g.DrawLine(white, r.Left + m, r.Top + m, r.Right - m, r.Bottom - m);
        g.DrawLine(red, r.Left + m, r.Top + m, r.Right - m, r.Bottom - m);
    }
}
