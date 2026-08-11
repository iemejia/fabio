import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import starlightBlog from "starlight-blog";

// The production site is served from the custom apex domain https://ismaelmejia.com
// under the /fabio subpath. CI sets SITE_URL/BASE_PATH explicitly (see docs.yml).
// The GITHUB_REPOSITORY-derived values remain a fallback for ad-hoc builds and
// forks (→ https://<owner>.github.io/<repo>), and localhost for local dev.
const [owner, repository] = (process.env.GITHUB_REPOSITORY ?? "").split("/");
const site = process.env.SITE_URL ?? (owner ? `https://${owner}.github.io` : "http://localhost:4321");
const base = process.env.BASE_PATH ?? (repository ? `/${repository}` : "/");

export default defineConfig({
  site,
  base,
  integrations: [
    starlight({
      title: "Fabio",
      description: "Agent-native command line interface for Microsoft Fabric.",
      favicon: "/favicon.svg",
      logo: {
        src: "./src/assets/fabio-square.png",
        alt: "Fabio",
      },
      customCss: ["./src/styles/docs.css"],
      plugins: [
        starlightBlog({
          title: "Blog",
          authors: {
            ismael: {
              name: "Ismael Mejía",
              title: "Fabio maintainer",
              url: "https://github.com/iemejia",
            },
          },
        }),
      ],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/iemejia/fabio",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/iemejia/fabio/edit/main/docs/",
      },
      sidebar: [
        {
          label: "Tutorials",
          items: [{ label: "Getting started", slug: "getting-started" }],
        },
        {
          label: "How-to guides",
          items: [{ autogenerate: { directory: "guides" } }],
        },
        {
          label: "Explanation",
          items: [{ autogenerate: { directory: "explanation" } }],
        },
        {
          label: "Reference",
          items: [
            { label: "CLI overview", slug: "reference" },
            { label: "Global flags", slug: "reference/global-flags" },
            {
              label: "Commands",
              items: [{ autogenerate: { directory: "reference/commands" } }],
              collapsed: true,
            },
          ],
        },
      ],
      pagefind: true,
      head: [
        {
          tag: "meta",
          attrs: { name: "theme-color", content: "#0d9488" },
        },
      ],
    }),
  ],
});
