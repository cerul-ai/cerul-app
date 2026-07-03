import { app, net, protocol } from "electron";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const appScheme = "app";
export const appHost = "cerul";
export const deepLinkSchemes = ["cerul", "cerul-app"];

export type AppProtocolOptions = {
  desktopDistDir: string;
  apiBaseUrl: () => string;
  cloudAccountOrigin: string;
};

export function registerPrivilegedAppScheme() {
  protocol.registerSchemesAsPrivileged([
    {
      scheme: appScheme,
      privileges: {
        standard: true,
        secure: true,
        supportFetchAPI: true,
        corsEnabled: true,
        stream: true,
      },
    },
  ]);
}

export function registerAppProtocol(options: AppProtocolOptions) {
  protocol.handle(appScheme, async (request) => {
    const url = new URL(request.url);
    if (url.hostname !== appHost) {
      return new Response("unknown app host", { status: 404 });
    }

    const dist = path.resolve(options.desktopDistDir);
    const pathname = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    const filePath = path.resolve(dist, pathname.replace(/^\/+/, ""));
    if (!isPathInsideDirectory(filePath, dist)) {
      return new Response("invalid app path", { status: 403 });
    }
    if (!fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
      return new Response("not found", { status: 404 });
    }

    const response = await net.fetch(pathToFileURL(filePath).toString());
    return withAppSecurityHeaders(response, filePath, options);
  });
}

export function registerDeepLinkProtocols() {
  for (const scheme of deepLinkSchemes) {
    if (app.isPackaged) {
      app.setAsDefaultProtocolClient(scheme);
    } else {
      app.setAsDefaultProtocolClient(scheme, process.execPath, [process.argv[1]].filter(Boolean));
    }
  }
}

export function firstDeepLinkArg(argv: string[]) {
  return argv.find((arg) => deepLinkSchemes.some((scheme) => arg.startsWith(`${scheme}://`)));
}

export function isDeepLinkScheme(scheme: string) {
  return deepLinkSchemes.includes(scheme);
}

export function isAppUrl(rawUrl: string) {
  try {
    const url = new URL(rawUrl);
    return url.protocol === `${appScheme}:` && url.hostname === appHost;
  } catch {
    return false;
  }
}

export function isExternalUrl(rawUrl: string) {
  try {
    const protocol = new URL(rawUrl).protocol;
    return protocol === "http:" || protocol === "https:" || protocol === "mailto:";
  } catch {
    return false;
  }
}

function withAppSecurityHeaders(
  response: Response,
  filePath: string,
  options: AppProtocolOptions,
) {
  if (!filePath.endsWith(".html")) {
    return response;
  }
  const headers = new Headers(response.headers);
  headers.set("Content-Security-Policy", contentSecurityPolicy(options));
  // Never cache index.html so it always references the current (content-hashed)
  // assets after a rebuild; the hashed assets themselves remain cacheable.
  headers.set("Cache-Control", "no-store");
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

function contentSecurityPolicy(options: AppProtocolOptions) {
  const apiBaseUrl = options.apiBaseUrl();
  return [
    "default-src 'self'",
    "script-src 'self'",
    "style-src 'self' 'unsafe-inline'",
    "font-src 'self' data:",
    `img-src 'self' ${appScheme}: ${apiBaseUrl} data: blob:`,
    `media-src 'self' ${apiBaseUrl} blob:`,
    `connect-src 'self' ${apiBaseUrl} ${options.cloudAccountOrigin}`,
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join("; ");
}

function isPathInsideDirectory(filePath: string, directory: string) {
  const relative = path.relative(directory, filePath);
  return relative === "" || (relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative));
}
