// 通用更新器交互测试台
// 这一段 JS 与 Rust 版 generic-updater 的状态机保持一致，用来在浏览器里
// 先验证 24h 自动检查、手动刷新、下载进度、SHA256 校验、重启与清理的交互。

const CONFIG = {
  feedUrl: "./feed/latest-version-windows-x64.json",
  intervalHours: 24,
  storageKey: "generic-updater-demo",
};

const el = {
  installedVersion: document.getElementById("installed-version"),
  intervalHours: document.getElementById("interval-hours"),
  stateBadge: document.getElementById("state-badge"),
  stateDetail: document.getElementById("state-detail"),
  nextCheck: document.getElementById("next-check-hint"),
  progressWrap: document.getElementById("progress-wrap"),
  progressFill: document.getElementById("progress-fill"),
  progressText: document.getElementById("progress-text"),
  progressBytes: document.getElementById("progress-bytes"),
  cardRelease: document.getElementById("card-release"),
  releaseVersion: document.getElementById("release-version"),
  releaseKind: document.getElementById("release-kind"),
  releaseNotes: document.getElementById("release-notes"),
  cardReady: document.getElementById("card-ready"),
  readyVersion: document.getElementById("ready-version"),
  cardFailed: document.getElementById("card-failed"),
  failedReason: document.getElementById("failed-reason"),
  log: document.getElementById("log"),
  btnCheck: document.getElementById("btn-check"),
  btnAuto: document.getElementById("btn-auto"),
  btnDownload: document.getElementById("btn-download"),
  btnRestart: document.getElementById("btn-restart"),
  btnDiscard: document.getElementById("btn-discard"),
  btnRetry: document.getElementById("btn-retry"),
  btnReset: document.getElementById("btn-reset"),
};

let app = loadApp();
let phase = "Idle";
let release = null;

function loadApp() {
  try {
    const raw = localStorage.getItem(CONFIG.storageKey);
    if (raw) {
      return Object.assign(
        { installedVersion: "1.0.0", lastCheckUnix: null, pending: null },
        JSON.parse(raw)
      );
    }
  } catch (error) {
    console.warn("读取本地状态失败", error);
  }
  return { installedVersion: "1.0.0", lastCheckUnix: null, pending: null };
}

function saveApp() {
  localStorage.setItem(CONFIG.storageKey, JSON.stringify(app));
}

function nowUnix() {
  return Math.floor(Date.now() / 1000);
}

function formatBytes(bytes) {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value.toFixed(value >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}

function compareVersion(left, right) {
  const parse = (text) =>
    String(text)
      .trim()
      .replace(/^v/i, "")
      .split(/[.+-]/)
      .slice(0, 3)
      .map((part) => parseInt(part, 10) || 0);
  const a = parse(left);
  const b = parse(right);
  for (let i = 0; i < 3; i += 1) {
    if (a[i] !== b[i]) return a[i] > b[i] ? 1 : -1;
  }
  return 0;
}

function autoCheckDue() {
  if (!app.lastCheckUnix) return true;
  return nowUnix() - app.lastCheckUnix >= CONFIG.intervalHours * 3600;
}

function renderCountdown() {
  if (autoCheckDue()) {
    el.nextCheck.textContent = "下次打开软件时会自动检查更新。";
    return;
  }
  const remain = CONFIG.intervalHours * 3600 - (nowUnix() - app.lastCheckUnix);
  const hours = Math.floor(remain / 3600);
  const minutes = Math.floor((remain % 3600) / 60);
  el.nextCheck.textContent = `距下次自动检查还有 ${hours} 小时 ${minutes} 分（手动检查不受限制）。`;
}

function emit(state, extra = {}) {
  const frame = {
    protocol: 2,
    type: "status",
    state,
    version: extra.version || "",
    notes: extra.notes || "",
    received: extra.received || 0,
    total: extra.total || 0,
    error: extra.error || null,
    automatic: Boolean(extra.automatic),
  };
  el.log.textContent += `${JSON.stringify(frame)}\n`;
  el.log.scrollTop = el.log.scrollHeight;
  return frame;
}

function setState(state, detail = "") {
  phase = state;
  el.stateBadge.textContent = state;
  el.stateBadge.className = `badge badge-${state.toLowerCase()}`;
  el.stateDetail.textContent = detail;
}

function showProgress(received, total) {
  el.progressWrap.hidden = false;
  const percent = total > 0 ? Math.min(100, (received / total) * 100) : 0;
  el.progressFill.style.width = `${percent.toFixed(1)}%`;
  el.progressText.textContent = total > 0 ? `${percent.toFixed(0)}%` : "…";
  el.progressBytes.textContent = `${formatBytes(received)} / ${total ? formatBytes(total) : "未知"}`;
}

function hideProgress() {
  el.progressWrap.hidden = true;
  el.progressFill.style.width = "0%";
  el.progressText.textContent = "0%";
  el.progressBytes.textContent = "";
}

function renderInstalled() {
  el.installedVersion.textContent = app.installedVersion;
}

function showRelease(feed) {
  release = feed;
  el.cardRelease.hidden = false;
  el.releaseVersion.textContent = `v${feed.version}`;
  el.releaseKind.textContent = feed.kind || "portable";
  el.releaseNotes.textContent = feed.notes || "（该版本没有填写变更说明）";
  el.cardReady.hidden = true;
  el.cardFailed.hidden = true;
}

function showReady(version) {
  el.cardReady.hidden = false;
  el.readyVersion.textContent = `v${version}`;
  el.cardRelease.hidden = true;
  el.cardFailed.hidden = true;
}

function showFailed(reason) {
  el.cardFailed.hidden = false;
  el.failedReason.textContent = reason;
}

function setBusy(busy) {
  for (const button of [el.btnCheck, el.btnAuto, el.btnDownload, el.btnRestart, el.btnRetry]) {
    if (button) button.disabled = busy;
  }
}

async function loadFeed() {
  const response = await fetch(`${CONFIG.feedUrl}?t=${Date.now()}`, { cache: "no-store" });
  if (!response.ok) throw new Error(`元数据请求失败: HTTP ${response.status}`);
  const feed = await response.json();

  if (!feed.version) throw new Error("元数据缺少 version");
  if (!Array.isArray(feed.packages) || feed.packages.length === 0) {
    throw new Error("元数据缺少 packages");
  }
  const pkg =
    feed.packages.find((item) => item.kind !== "installer" && item.variant === "portable") ||
    feed.packages.find((item) => item.kind !== "installer");
  if (!pkg) throw new Error("元数据里没有可用的更新包");
  if (!/^[0-9a-f]{64}$/i.test(pkg.sha256 || "")) throw new Error("元数据 sha256 非法");

  return {
    version: feed.version,
    notes: feed.notes || "",
    kind: pkg.kind || pkg.variant || "portable",
    path: pkg.path,
    size: pkg.size || 0,
    sha256: pkg.sha256.toLowerCase(),
    platform: feed.platform || "",
  };
}

function assetUrl(path) {
  return new URL(path, new URL(CONFIG.feedUrl, window.location.href)).href;
}

async function check({ automatic = false } = {}) {
  if (automatic && !autoCheckDue()) {
    setState("Idle", "距上次检查不足设定间隔，跳过这次自动检查");
    return;
  }

  setBusy(true);
  el.cardFailed.hidden = true;
  setState("Checking", automatic ? "自动检查中…" : "手动检查中…");
  emit("Checking", { automatic });

  try {
    const feed = await loadFeed();
    app.lastCheckUnix = nowUnix();
    saveApp();
    renderCountdown();

    if (compareVersion(feed.version, app.installedVersion) > 0) {
      showRelease(feed);
      setState("Available", `发现新版本 v${feed.version}`);
      emit("Available", { version: feed.version, notes: feed.notes, automatic });
    } else {
      el.cardRelease.hidden = true;
      setState("Idle", `已是最新版本（v${app.installedVersion}）`);
      emit("Idle", { version: app.installedVersion, automatic });
    }
  } catch (error) {
    setState("Failed", "检查失败");
    showFailed(error.message);
    emit("Failed", { error: error.message });
  } finally {
    setBusy(false);
    setTimeout(() => {
      if (phase === "Checking") setState("Idle", "");
    }, 200);
  }
}

async function download() {
  if (!release) return;
  setBusy(true);
  el.cardFailed.hidden = true;
  el.cardRelease.hidden = true;
  hideProgress();
  setState("Downloading", `正在下载 v${release.version}`);
  emit("Downloading", { version: release.version, notes: release.notes });

  try {
    const url = assetUrl(release.path);
    const response = await fetch(url, { cache: "no-store" });
    if (!response.ok) throw new Error(`下载失败: HTTP ${response.status}`);

    const total = Number(response.headers.get("content-length")) || release.size || 0;
    const reader = response.body.getReader();
    const chunks = [];
    let received = 0;

    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
      received += value.byteLength;
      showProgress(received, total);
      if (received === value.byteLength || received % (64 * 1024) < value.byteLength) {
        emit("Downloading", {
          version: release.version,
          notes: release.notes,
          received,
          total,
        });
      }
    }

    if (total && received !== total) throw new Error(`大小不符: 期望 ${total}，实际 ${received}`);

    setState("Verifying", "校验 SHA256…");
    emit("Verifying", { version: release.version, notes: release.notes });

    const buffer = new Uint8Array(received);
    let offset = 0;
    for (const chunk of chunks) {
      buffer.set(chunk, offset);
      offset += chunk.byteLength;
    }
    const digest = await crypto.subtle.digest("SHA-256", buffer);
    const actual = Array.from(new Uint8Array(digest))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");

    if (actual !== release.sha256) {
      throw new Error(`SHA256 不一致：期望 ${release.sha256.slice(0, 12)}…，实际 ${actual.slice(0, 12)}…`);
    }

    app.pending = { version: release.version, notes: release.notes };
    saveApp();
    setState("Ready", `v${release.version} 已就绪，等待重启`);
    emit("Ready", { version: release.version, notes: release.notes, received: 1, total: 1 });
    showReady(release.version);
  } catch (error) {
    setState("Failed", "下载失败");
    showFailed(error.message);
    emit("Failed", { version: release.version, error: error.message });
    el.cardRelease.hidden = false;
  } finally {
    setBusy(false);
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function applyUpdate() {
  if (!app.pending) return;
  const target = app.pending.version;
  setBusy(true);
  setState("Applying", "备份旧文件 → 覆盖新文件 → 清理缓存");
  emit("Applying", { version: target, notes: app.pending.notes || "" });

  try {
    await sleep(400);
    el.stateDetail.textContent = "备份旧文件…";
    await sleep(400);
    el.stateDetail.textContent = "覆盖新文件…";
    await sleep(500);
    el.stateDetail.textContent = "清理下载缓存与暂存目录…";
    await sleep(400);

    app.installedVersion = target;
    app.pending = null;
    saveApp();
    renderInstalled();
    renderCountdown();
    hideProgress();
    el.cardReady.hidden = true;
    el.cardRelease.hidden = true;

    emit("Ready", { version: target, notes: "", received: 1, total: 1 });
    setState("Idle", `已更新到 v${target}，正在拉起新版…`);
    el.log.textContent += `${JSON.stringify({ type: "relaunch", exe: "app.exe", detached: true })}\n`;
    await sleep(900);
    el.stateDetail.textContent = `已更新到 v${target}`;
  } catch (error) {
    showFailed(error.message);
    setState("Failed", "应用更新失败");
    emit("Failed", { version: target, error: error.message });
  } finally {
    setBusy(false);
  }
}

function discard() {
  app.pending = null;
  saveApp();
  el.cardReady.hidden = true;
  el.cardRelease.hidden = false;
  setState("Idle", "已放弃本次更新");
}

function resetDemo() {
  localStorage.removeItem(CONFIG.storageKey);
  window.location.reload();
}

// ---- 事件绑定 ----
el.intervalHours.textContent = String(CONFIG.intervalHours);

el.btnCheck.addEventListener("click", () => check({ automatic: false }));
el.btnAuto.addEventListener("click", () => check({ automatic: true }));
el.btnDownload.addEventListener("click", download);
el.btnRestart.addEventListener("click", applyUpdate);
el.btnDiscard.addEventListener("click", discard);
el.btnRetry.addEventListener("click", () => {
  el.cardFailed.hidden = true;
  check({ automatic: false });
});
el.btnReset.addEventListener("click", resetDemo);
el.btnRestart.disabled = false;

renderInstalled();
renderCountdown();
setState("Idle", "等待操作");
if (app.pending) {
  showReady(app.pending.version);
  setState("Ready", `v${app.pending.version} 已就绪，等待重启`);
}

// 模拟"用户打开软件"：启动后先按间隔判断要不要自动检查。
setTimeout(() => {
  if (phase === "Idle") check({ automatic: true });
}, 500);
