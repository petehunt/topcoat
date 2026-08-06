const patchSelector = "template[data-topcoat-defer-patch]";
const resourceSelector = "template[data-topcoat-resource]";
const jsonSelector = "template[data-topcoat-json]";
const streamedSelector = [patchSelector, resourceSelector, jsonSelector].join(",");

const topcoat = (globalThis.topcoat ??= {});
const jsonValues = (topcoat.__jsonValues ??= new Map());
const jsonWaiters = (topcoat.__jsonWaiters ??= new Map());
const loadedResources = (topcoat.__loadedResources ??= new Set());
const react = (topcoat.react ??= {});
const reactComponents = (react.__components ??= new Map());
const reactWaiters = (react.__waiters ??= new Map());
const pendingReact = new Map();
const mountedReact = new Map();

react.register = function register(name, mount) {
  const existing = reactComponents.get(name);
  if (existing && existing !== mount) {
    throw new Error(`React component ${JSON.stringify(name)} is already registered`);
  }
  reactComponents.set(name, mount);
  for (const resolve of reactWaiters.get(name) ?? []) resolve(mount);
  reactWaiters.delete(name);
};

function streamedJson(key, signal) {
  if (jsonValues.has(key)) return Promise.resolve(jsonValues.get(key));
  return new Promise((resolve, reject) => {
    const waiters = jsonWaiters.get(key) ?? [];
    const receive = (value) => {
      signal?.removeEventListener("abort", cancel);
      resolve(value);
    };
    const cancel = () => {
      const remaining = (jsonWaiters.get(key) ?? []).filter(
        (waiter) => waiter !== receive,
      );
      if (remaining.length) jsonWaiters.set(key, remaining);
      else jsonWaiters.delete(key);
      reject(signal.reason);
    };
    waiters.push(receive);
    jsonWaiters.set(key, waiters);
    signal?.addEventListener("abort", cancel, { once: true });
    if (signal?.aborted) cancel();
  });
}

topcoat.json = function json(key) {
  return streamedJson(key);
};

function applyPatch(patch) {
  const id = patch.dataset.topcoatDeferPatch;
  const starts = document.querySelectorAll(
    `template[data-topcoat-defer-start="${id}"]`,
  );

  for (const start of starts) {
    let end = start.nextSibling;
    while (
      end &&
      !(
        end instanceof HTMLTemplateElement &&
        end.dataset.topcoatDeferEnd === id
      )
    ) {
      end = end.nextSibling;
    }
    if (!end) continue;

    let node = start.nextSibling;
    while (node !== end) {
      const next = node.nextSibling;
      node.remove();
      node = next;
    }
    end.replaceWith(patch.content.cloneNode(true));
    start.remove();
  }

  patch.remove();
}

function loadResource(template) {
  const { topcoatResource: kind, topcoatResourceKey: key } = template.dataset;
  if (!loadedResources.has(key)) {
    loadedResources.add(key);
    const resource = document.createElement(kind === "module" ? "script" : "link");
    resource.dataset.topcoatResourceKey = key;
    if (kind === "module") {
      resource.type = "module";
      resource.src = template.dataset.topcoatResourceSrc;
    } else {
      resource.rel = "stylesheet";
      resource.href = template.dataset.topcoatResourceSrc;
    }
    document.head.append(resource);
  }
  template.remove();
}

function receiveJson(template) {
  const key = template.dataset.topcoatJson;
  const value = JSON.parse(template.content.textContent);
  jsonValues.set(key, value);
  for (const resolve of jsonWaiters.get(key) ?? []) resolve(value);
  jsonWaiters.delete(key);
  template.remove();
}

function registeredReact(name, signal) {
  if (reactComponents.has(name)) {
    return Promise.resolve(reactComponents.get(name));
  }
  return new Promise((resolve, reject) => {
    const waiters = reactWaiters.get(name) ?? [];
    const register = (mount) => {
      signal.removeEventListener("abort", cancel);
      resolve(mount);
    };
    const cancel = () => {
      const remaining = (reactWaiters.get(name) ?? []).filter(
        (waiter) => waiter !== register,
      );
      if (remaining.length) reactWaiters.set(name, remaining);
      else reactWaiters.delete(name);
      reject(signal.reason);
    };
    waiters.push(register);
    reactWaiters.set(name, waiters);
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
  });
}

async function mountReact(element) {
  if (element.dataset.topcoatReactMounting) return;
  element.dataset.topcoatReactMounting = "true";
  const controller = new AbortController();
  pendingReact.set(element, controller);

  try {
    const name = element.dataset.topcoatReact;
    const [mount, payload] = await Promise.all([
      registeredReact(name, controller.signal),
      streamedJson(element.dataset.topcoatReactPayload, controller.signal),
    ]);
    const entries = await Promise.all(
      payload.preloads.map(async ({ key, jsonKey }) => [
        key,
        await streamedJson(jsonKey, controller.signal),
      ]),
    );
    if (!element.isConnected) return;
    const cleanup = await mount({
      element,
      props: payload.props,
      fallback: Object.fromEntries(entries),
    });
    if (!element.isConnected) {
      cleanup?.();
      return;
    }
    if (typeof cleanup === "function") mountedReact.set(element, cleanup);
    element.dataset.topcoatReactMounted = "true";
  } catch (error) {
    if (controller.signal.aborted) return;
    delete element.dataset.topcoatReactMounting;
    queueMicrotask(() => {
      throw error;
    });
  } finally {
    pendingReact.delete(element);
  }
}

function cleanupReact(root) {
  for (const [element, controller] of pendingReact) {
    if (element === root || root.contains?.(element)) {
      controller.abort(new DOMException("React island was removed", "AbortError"));
      pendingReact.delete(element);
    }
  }
  for (const [element, cleanup] of mountedReact) {
    if (element === root || root.contains?.(element)) {
      cleanup();
      mountedReact.delete(element);
    }
  }
}

function applyStreamed(template) {
  if (template.matches(patchSelector)) applyPatch(template);
  else if (template.matches(resourceSelector)) loadResource(template);
  else receiveJson(template);
}

function scan(node) {
  if (!(node instanceof Element)) return;
  if (node.matches(streamedSelector)) applyStreamed(node);
  node.querySelectorAll(streamedSelector).forEach(applyStreamed);
  if (node.matches("[data-topcoat-react]")) mountReact(node);
  node.querySelectorAll("[data-topcoat-react]").forEach(mountReact);
}

document
  .querySelectorAll("link[data-topcoat-resource-key],script[data-topcoat-resource-key]")
  .forEach((resource) => loadedResources.add(resource.dataset.topcoatResourceKey));
document.querySelectorAll(streamedSelector).forEach(applyStreamed);
document.querySelectorAll("[data-topcoat-react]").forEach(mountReact);
new MutationObserver((records) => {
  for (const record of records) {
    record.removedNodes.forEach(cleanupReact);
    record.addedNodes.forEach(scan);
  }
}).observe(document.documentElement, { childList: true, subtree: true });
