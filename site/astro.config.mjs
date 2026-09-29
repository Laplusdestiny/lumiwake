// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// GitHub Pages（https://laplusdestiny.github.io/lumiwake/）で公開する
export default defineConfig({
  site: "https://laplusdestiny.github.io",
  base: "/lumiwake",
  integrations: [
    starlight({
      title: "Lumiwake",
      description: "キーボード操作で画像をすばやくフォルダへ振り分ける、軽量なデスクトップアプリ",
      favicon: "/favicon.svg",
      customCss: ["./src/styles/custom.css"],
      defaultLocale: "root",
      locales: {
        root: { label: "日本語", lang: "ja" },
        en: { label: "English", lang: "en" },
      },
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/laplusdestiny/lumiwake" }],
      sidebar: [
        {
          label: "ガイド",
          translations: { en: "Guides" },
          items: [
            { slug: "guides/install" },
            { slug: "guides/usage" },
            { slug: "guides/keys" },
          ],
        },
        {
          label: "リファレンス",
          translations: { en: "Reference" },
          items: [{ slug: "reference/config" }, { slug: "reference/safety" }],
        },
      ],
    }),
  ],
});
