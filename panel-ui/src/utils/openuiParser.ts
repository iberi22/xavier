/**
 * Safe OpenUI markup parser.
 * Converts openui_lang pseudo-XML markup to typed OpenUiNode tree.
 * Unknown tags degrade to safe text nodes; malformed input returns null.
 * Never uses dangerouslySetInnerHTML or executes scripts.
 */

export type OpenUiNodeType =
	| "SectionBlock"
	| "Card"
	| "TextContent"
	| "ListBlock"
	| "QuestionCard"
	| "DecisionCard"
	| "ProjectCard"
	| "Accordion"
	| "TextCallout"
	| "InfoCard"
	| "text";

export interface BaseNode {
	id: string;
}

export interface SectionBlockNode extends BaseNode {
	type: "SectionBlock";
	title?: string;
	description?: string;
	children: OpenUiNode[];
}

export interface CardNode extends BaseNode {
	type: "Card";
	title?: string;
	description?: string;
	children: OpenUiNode[];
}

export interface TextContentNode extends BaseNode {
	type: "TextContent";
	content: string;
}

export interface ListItem {
	id: string;
	title?: string;
	description?: string;
	meta?: string;
}

export interface ListBlockNode extends BaseNode {
	type: "ListBlock";
	items: ListItem[];
	rawItems?: string;
}

export interface QuestionCardNode extends BaseNode {
	type: "QuestionCard";
	question: string;
}

export interface DecisionCardNode extends BaseNode {
	type: "DecisionCard";
	adr: string;
}

export interface ProjectCardNode extends BaseNode {
	type: "ProjectCard";
	project: string;
}

export interface AccordionItem {
	id: string;
	title?: string;
	content?: string;
}

export interface AccordionNode extends BaseNode {
	type: "Accordion";
	items: AccordionItem[];
	rawItems?: string;
}

export interface TextCalloutNode extends BaseNode {
	type: "TextCallout";
	variant?: "warning" | "success" | "info" | "default" | "error" | string;
	title?: string;
	content: string;
}

export interface InfoCardNode extends BaseNode {
	type: "InfoCard";
	title: string;
	value: string;
}

export interface RawTextNode extends BaseNode {
	type: "text";
	content: string;
}

export type OpenUiNode =
	| SectionBlockNode
	| CardNode
	| TextContentNode
	| ListBlockNode
	| QuestionCardNode
	| DecisionCardNode
	| ProjectCardNode
	| AccordionNode
	| TextCalloutNode
	| InfoCardNode
	| RawTextNode;

interface IdCounter {
	count: number;
}

function coerceField(v: unknown): string | undefined {
	return typeof v === "string" ? v : v == null ? undefined : String(v);
}

export function isNodeVisible(node: OpenUiNode): boolean {
	switch (node.type) {
		case "SectionBlock":
		case "Card":
			return (
				Boolean(node.title?.trim()) ||
				Boolean(node.description?.trim()) ||
				node.children.some(isNodeVisible)
			);
		case "TextContent":
			return Boolean(node.content?.trim());
		case "ListBlock":
			return (
				node.items.length > 0 &&
				node.items.some(
					(item) =>
						Boolean(item.title?.trim()) ||
						Boolean(item.description?.trim()) ||
						Boolean(item.meta?.trim()),
				)
			);
		case "Accordion":
			return (
				node.items.length > 0 &&
				node.items.some(
					(item) =>
						Boolean(item.title?.trim()) || Boolean(item.content?.trim()),
				)
			);
		case "QuestionCard":
			return Boolean(node.question?.trim());
		case "DecisionCard":
			return Boolean(node.adr?.trim());
		case "ProjectCard":
			return Boolean(node.project?.trim());
		case "InfoCard":
			return Boolean(node.title?.trim()) || Boolean(node.value?.trim());
		case "TextCallout":
			return Boolean(node.title?.trim()) || Boolean(node.content?.trim());
		case "text":
			return Boolean(node.content?.trim());
		default:
			return false;
	}
}

function parseListItems(itemsAttr: string, counter: IdCounter): ListItem[] {
	if (!itemsAttr || !itemsAttr.trim()) return [];
	try {
		const parsed = JSON.parse(itemsAttr);
		if (Array.isArray(parsed)) {
			return parsed
				.map((item) => {
					counter.count += 1;
					const id = `item-${counter.count}`;
					if (typeof item === "string") return { id, title: item };
					if (typeof item === "object" && item !== null) {
						const rec = item as Record<string, unknown>;
						return {
							id,
							title: coerceField(rec.title ?? rec.name),
							description: coerceField(
								rec.description ?? rec.desc ?? rec.content,
							),
							meta: coerceField(rec.meta ?? rec.extra),
						};
					}
					return { id, title: String(item) };
				})
				.filter(
					(item) =>
						Boolean(item.title?.trim()) ||
						Boolean(item.description?.trim()) ||
						Boolean(item.meta?.trim()),
				);
		}
	} catch {
		// If not valid JSON, treat lines as list items
		return itemsAttr
			.split("\n")
			.map((s) => s.trim())
			.filter(Boolean)
			.map((title) => {
				counter.count += 1;
				return { id: `item-${counter.count}`, title };
			});
	}
	return [];
}

function parseAccordionItems(
	itemsAttr: string,
	counter: IdCounter,
): AccordionItem[] {
	if (!itemsAttr || !itemsAttr.trim()) return [];
	try {
		const parsed = JSON.parse(itemsAttr);
		if (Array.isArray(parsed)) {
			return parsed
				.map((item) => {
					counter.count += 1;
					const id = `accordion-item-${counter.count}`;
					if (typeof item === "string") return { id, title: item, content: "" };
					if (typeof item === "object" && item !== null) {
						const rec = item as Record<string, unknown>;
						const rawTitle = rec.title ?? rec.name ?? rec.path;
						const rawContent = rec.content ?? rec.description ?? rec.desc;
						const title = coerceField(rawTitle);
						const content = coerceField(rawContent);
						return {
							id,
							title: title ?? (content ? "Item" : undefined),
							content: content ?? "",
						};
					}
					return { id, title: String(item), content: "" };
				})
				.filter(
					(item) =>
						Boolean(item.title?.trim()) || Boolean(item.content?.trim()),
				);
		}
	} catch {
		counter.count += 1;
		return [
			{
				id: `accordion-item-${counter.count}`,
				title: "Details",
				content: itemsAttr,
			},
		];
	}
	return [];
}

function parseNode(node: Node, counter: IdCounter): OpenUiNode | null {
	counter.count += 1;
	const id = `node-${counter.count}`;

	if (node.nodeType === Node.TEXT_NODE) {
		const text = node.textContent?.trim();
		if (!text) return null;
		return { id, type: "text", content: text };
	}

	if (node.nodeType !== Node.ELEMENT_NODE) {
		return null;
	}

	const el = node as Element;
	const tagName = el.tagName.toLowerCase();

	switch (tagName) {
		case "sectionblock": {
			const title = el.getAttribute("title") ?? undefined;
			const description = el.getAttribute("description") ?? undefined;
			const children = parseChildren(el, counter).filter(isNodeVisible);
			if (!title?.trim() && !description?.trim() && children.length === 0) {
				return null;
			}
			return { id, type: "SectionBlock", title, description, children };
		}
		case "card": {
			const title = el.getAttribute("title") ?? undefined;
			const description = el.getAttribute("description") ?? undefined;
			const children = parseChildren(el, counter).filter(isNodeVisible);
			if (!title?.trim() && !description?.trim() && children.length === 0) {
				return null;
			}
			return { id, type: "Card", title, description, children };
		}
		case "textcontent": {
			const content =
				el.getAttribute("content") ?? el.textContent?.trim() ?? "";
			if (!content.trim()) return null;
			return { id, type: "TextContent", content };
		}
		case "listblock": {
			const itemsAttr = el.getAttribute("items") ?? "";
			const items = parseListItems(itemsAttr, counter);
			if (items.length === 0) return null;
			return { id, type: "ListBlock", items, rawItems: itemsAttr || undefined };
		}
		case "questioncard": {
			const question =
				el.getAttribute("question") ?? el.textContent?.trim() ?? "";
			return { id, type: "QuestionCard", question };
		}
		case "decisioncard": {
			const adr =
				el.getAttribute("adr") ??
				el.getAttribute("title") ??
				el.textContent?.trim() ??
				"";
			return { id, type: "DecisionCard", adr };
		}
		case "projectcard": {
			const project =
				el.getAttribute("project") ??
				el.getAttribute("name") ??
				el.textContent?.trim() ??
				"";
			return { id, type: "ProjectCard", project };
		}
		case "accordion": {
			const itemsAttr = el.getAttribute("items") ?? "";
			const items = parseAccordionItems(itemsAttr, counter);
			if (items.length === 0) return null;
			return { id, type: "Accordion", items, rawItems: itemsAttr || undefined };
		}
		case "textcallout": {
			const variant = el.getAttribute("variant") ?? "info";
			const title = el.getAttribute("title") ?? undefined;
			const content =
				el.getAttribute("content") ??
				el.getAttribute("text") ??
				el.textContent?.trim() ??
				"";
			return { id, type: "TextCallout", variant, title, content };
		}
		case "infocard": {
			const title = el.getAttribute("title") ?? "";
			const value =
				el.getAttribute("value") ??
				el.getAttribute("content") ??
				el.textContent?.trim() ??
				"";
			return { id, type: "InfoCard", title, value };
		}
		default: {
			// Unknown tags degrade to text
			const content = el.textContent?.trim();
			if (!content) return null;
			return { id, type: "text", content };
		}
	}
}

function parseChildren(parent: Element, counter: IdCounter): OpenUiNode[] {
	const result: OpenUiNode[] = [];
	for (let i = 0; i < parent.childNodes.length; i++) {
		const child = parent.childNodes[i];
		const parsed = parseNode(child, counter);
		if (parsed) {
			result.push(parsed);
		}
	}
	return result;
}

/**
 * Safely parse openui_lang markup to an array of OpenUiNode.
 * Returns null if markup is malformed, empty, or invalid.
 */
export function parseOpenUi(
	markup: string | null | undefined,
): OpenUiNode[] | null {
	if (markup == null || typeof markup !== "string") {
		return null;
	}

	const trimmed = markup.trim();
	if (!trimmed.includes("<")) {
		return null;
	}

	// Pre-normalize raw ampersands that are not valid XML entities
	const sanitized = trimmed.replace(
		/&(?!(amp|lt|gt|quot|apos|#\d+|#[xX][0-9a-fA-F]+);)/g,
		"&amp;",
	);

	const parser = typeof DOMParser !== "undefined" ? new DOMParser() : null;
	if (!parser) {
		return null;
	}

	let doc: Document;
	try {
		doc = parser.parseFromString(`<root>${sanitized}</root>`, "text/xml");
	} catch {
		return null;
	}

	if (
		doc.querySelector("parsererror") != null ||
		doc.getElementsByTagName("parsererror").length > 0
	) {
		return null;
	}

	const root = doc.documentElement;
	if (!root) {
		return null;
	}

	const counter: IdCounter = { count: 0 };
	const nodes = parseChildren(root, counter).filter(isNodeVisible);
	if (nodes.length === 0) {
		return null;
	}

	return nodes;
}
