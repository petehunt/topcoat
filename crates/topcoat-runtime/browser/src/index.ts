import { Runtime } from "./runtime";
import { scan } from "./scan";

const runtime = new Runtime();
runtime.start(document.body);

type TopcoatGlobal = typeof globalThis & {
	__topcoatScan?: (root: Node, from: Node | null, to: Node | null) => void;
};

(globalThis as TopcoatGlobal).__topcoatScan = (root, from, to) => {
	scan(root, from, to, runtime.rootScope);
};
