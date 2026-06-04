type AppBackgroundProps = {
  isDark: boolean;
};

export function AppBackground({ isDark }: AppBackgroundProps) {
  return (
    <>
      <div className="pointer-events-none absolute inset-0 overflow-hidden hidden md:block">
        <div
          className={`absolute -top-32 -left-32 h-96 w-96 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-br from-[rgba(255,255,255,0.05)] via-[rgba(168,162,158,0.03)] to-transparent"
              : "bg-gradient-to-br from-[rgba(244,240,232,0.42)] via-[rgba(232,227,219,0.18)] to-transparent"
          }`}
          style={{ opacity: isDark ? 0.68 : 0.82 }}
        />
        <div
          className={`absolute top-1/4 -right-24 h-80 w-80 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-bl from-[rgba(226,232,240,0.04)] via-[rgba(148,163,184,0.025)] to-transparent"
              : "bg-gradient-to-bl from-[rgba(233,236,240,0.36)] via-[rgba(222,227,232,0.14)] to-transparent"
          }`}
          style={{ opacity: isDark ? 0.58 : 0.7 }}
        />
        <div
          className={`absolute -bottom-20 left-1/3 h-72 w-72 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-tr from-[rgba(231,229,228,0.04)] via-[rgba(255,255,255,0.018)] to-transparent"
              : "bg-gradient-to-tr from-[rgba(236,233,228,0.3)] via-[rgba(228,232,236,0.12)] to-transparent"
          }`}
          style={{ opacity: isDark ? 0.54 : 0.62 }}
        />
      </div>

      <div
        className="pointer-events-none absolute inset-0 hidden md:block"
        style={{
          backgroundImage:
            isDark
              ? `radial-gradient(circle at top, rgba(255,255,255,0.035), transparent 52%), linear-gradient(180deg, rgba(17,17,19,0.82) 0%, rgba(10,10,11,0.92) 100%), url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E")`
              : `linear-gradient(180deg, rgba(252,250,246,0.96) 0%, rgba(247,245,241,0.92) 100%), url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E")`,
          backgroundRepeat: "no-repeat, no-repeat, repeat",
          backgroundSize: "cover, cover, 256px 256px",
          opacity: isDark ? 0.024 : 0.02,
        }}
      />
    </>
  );
}
