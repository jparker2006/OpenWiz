// A static export (`out/`), served same-origin under `/` by openwiz-server
// The app calls `/api/v1/...` with relative URLs only.
/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'export',
  images: { unoptimized: true },
  trailingSlash: false,
  poweredByHeader: false,
  reactStrictMode: true,
  productionBrowserSourceMaps: false,
  webpack(config) {
    // webpack's `global` runtime falls back to Function("return this")() when
    // globalThis is missing; a CSP without 'unsafe-eval' refuses that, and every
    // browser we support has globalThis. So the runtime is just globalThis.
    // Next's bundled webpack ignores output.environment.globalThis here.
    config.plugins.push({
      apply(compiler) {
        compiler.hooks.compilation.tap('OpenWizGlobalThis', (compilation) => {
          compilation.hooks.runtimeModule.tap('OpenWizGlobalThis', (module) => {
            if (module.name === 'global') module.generate = () => '__webpack_require__.g = globalThis;';
          });
        });
      },
    });
    return config;
  },
};

export default nextConfig;
