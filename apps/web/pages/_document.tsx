import { Head, Html, Main, NextScript } from 'next/document';

// The fonts are bundled (public/fonts, OFL 1.1). Nothing is loaded from
// another origin.
export default function Document() {
  return (
    <Html lang="en">
      <Head>
        <meta name="color-scheme" content="dark" />
        <link rel="preload" href="/fonts/Geist-var.woff2" as="font" type="font/woff2" crossOrigin="anonymous" />
        <link rel="preload" href="/fonts/GeistMono-var.woff2" as="font" type="font/woff2" crossOrigin="anonymous" />
      </Head>
      <body>
        <Main />
        <NextScript />
      </body>
    </Html>
  );
}
