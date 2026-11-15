// 演示用静态服务器。
//
// 为什么不用 `python -m http.server`：本机安全软件会拦截 URL 以 .zip 结尾的浏览器请求
// （直接返回 204 空响应），导致 demo 的下载步骤必然失败。这里用 /feed/portable 这个
// 无扩展名路径返回同一份字节，真实更新器（reqwest）不受这个浏览器侧拦截影响。
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL(".", import.meta.url)));
const port = Number(process.env.PORT || 8124);
const assetPath = join(root, "feed", "myapp_windows-x64-portable.zip");

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".sha256": "text/plain; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
};

async function sendFile(res, path, type) {
  const body = await readFile(path);
  res.writeHead(200, {
    "Content-Type": type,
    "Content-Length": String(body.length),
    "Cache-Control": "no-store",
  });
  res.end(body);
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://127.0.0.1:${port}`);

  if (url.pathname === "/feed/portable") {
    try {
      await sendFile(res, assetPath, "application/octet-stream");
    } catch (error) {
      res.writeHead(500).end(String(error));
    }
    return;
  }

  const relative = normalize(decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname));
  const target = join(root, relative);
  if (!target.startsWith(root)) {
    res.writeHead(403).end("forbidden");
    return;
  }

  try {
    await sendFile(res, target, types[extname(target)] || "application/octet-stream");
  } catch {
    res.writeHead(404).end("not found");
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`demo: http://127.0.0.1:${port}/`);
});
