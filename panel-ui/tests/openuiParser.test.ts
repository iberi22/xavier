import { describe, expect, it } from "vitest";
import { parseOpenUi } from "../src/utils/openuiParser";

describe("openuiParser", () => {
	describe("valid tags and attributes", () => {
		it("parses SectionBlock with title, description, and children", () => {
			const markup = `
        <SectionBlock title="Internal Panel" description="Agent outputs">
          <TextContent content="Hello from Section" />
        </SectionBlock>
      `;
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes).toHaveLength(1);

			const section = nodes?.[0];
			expect(section?.type).toBe("SectionBlock");
			if (section?.type === "SectionBlock") {
				expect(section.title).toBe("Internal Panel");
				expect(section.description).toBe("Agent outputs");
				expect(section.children).toHaveLength(1);
				expect(section.children[0].type).toBe("TextContent");
			}
		});

		it("parses Card with title and description", () => {
			const markup =
				'<Card title="Card Title" description="Card Desc"><TextContent content="Inner text" /></Card>';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0].type).toBe("Card");
			if (nodes?.[0].type === "Card") {
				expect(nodes[0].title).toBe("Card Title");
				expect(nodes[0].description).toBe("Card Desc");
				expect(nodes[0].children).toHaveLength(1);
			}
		});

		it("parses TextContent with content attribute or inner text", () => {
			const markup1 = '<TextContent content="Attribute Content" />';
			const nodes1 = parseOpenUi(markup1);
			expect(nodes1).not.toBeNull();
			expect(nodes1?.[0]).toEqual(
				expect.objectContaining({
					type: "TextContent",
					content: "Attribute Content",
				}),
			);

			const markup2 = "<TextContent>Inner text content</TextContent>";
			const nodes2 = parseOpenUi(markup2);
			expect(nodes2).not.toBeNull();
			expect(nodes2?.[0]).toEqual(
				expect.objectContaining({
					type: "TextContent",
					content: "Inner text content",
				}),
			);
		});

		it("parses ListBlock with JSON items attribute", () => {
			const itemsJson = JSON.stringify([
				{
					title: "Confidence",
					description: "85% answer confidence",
					meta: "score 0.85",
				},
				{ title: "Latency", description: "42ms" },
			]);
			const markup = `<ListBlock items='${itemsJson.replace(/'/g, "&apos;")}' />`;
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0].type).toBe("ListBlock");
			if (nodes?.[0].type === "ListBlock") {
				expect(nodes[0].items).toHaveLength(2);
				expect(nodes[0].items[0]).toEqual(
					expect.objectContaining({
						title: "Confidence",
						description: "85% answer confidence",
						meta: "score 0.85",
					}),
				);
				expect(nodes[0].items[1].title).toBe("Latency");
			}
		});

		it("parses QuestionCard, DecisionCard, and ProjectCard", () => {
			const markup = `
        <QuestionCard question="How does episodic memory work?" />
        <DecisionCard adr="ADR-036: Hybrid Vector Index" />
        <ProjectCard project="xavier-memory-engine" />
      `;
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes).toHaveLength(3);

			expect(nodes?.[0]).toEqual(
				expect.objectContaining({
					type: "QuestionCard",
					question: "How does episodic memory work?",
				}),
			);
			expect(nodes?.[1]).toEqual(
				expect.objectContaining({
					type: "DecisionCard",
					adr: "ADR-036: Hybrid Vector Index",
				}),
			);
			expect(nodes?.[2]).toEqual(
				expect.objectContaining({
					type: "ProjectCard",
					project: "xavier-memory-engine",
				}),
			);
		});

		it("parses Accordion with JSON items attribute", () => {
			const accordionJson = JSON.stringify([
				{ title: "memory/doc-1", content: "Excerpt of document 1" },
				{ title: "memory/doc-2", content: "Excerpt of document 2" },
			]);
			const markup = `<Accordion items='${accordionJson.replace(/'/g, "&apos;")}' />`;
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0].type).toBe("Accordion");
			if (nodes?.[0].type === "Accordion") {
				expect(nodes[0].items).toHaveLength(2);
				expect(nodes[0].items[0].title).toBe("memory/doc-1");
				expect(nodes[0].items[0].content).toBe("Excerpt of document 1");
			}
		});

		it("parses TextCallout with variant, title, and content", () => {
			const markup =
				'<TextCallout variant="warning" title="Low Confidence" content="Evidence is partial" />';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0]).toEqual(
				expect.objectContaining({
					type: "TextCallout",
					variant: "warning",
					title: "Low Confidence",
					content: "Evidence is partial",
				}),
			);
		});

		it("parses InfoCard with title and value", () => {
			const markup = '<InfoCard title="Status" value="Ready" />';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0]).toEqual(
				expect.objectContaining({
					type: "InfoCard",
					title: "Status",
					value: "Ready",
				}),
			);
		});

		it("correctly unescapes XML entities in attributes", () => {
			const markup =
				'<Card title="Xavier &amp; Jules" description="A &gt; B &amp;&amp; C &lt; D"><TextContent content="Quote &quot;test&quot;" /></Card>';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			if (nodes?.[0].type === "Card") {
				expect(nodes[0].title).toBe("Xavier & Jules");
				expect(nodes[0].description).toBe("A > B && C < D");
				if (nodes[0].children[0].type === "TextContent") {
					expect(nodes[0].children[0].content).toBe('Quote "test"');
				}
			}
		});

		it("parses realistic output from backend ui_render.rs", () => {
			const markup = `
        <SectionBlock title="Xavier Internal Panel" description="Reasoning output">
          <Card title="Xavier Response" description="Rendered by internal agent">
            <TextContent content="Cognitive core initialized." />
          </Card>
          <Card title="System Signals" description="Reasoning signals">
            <ListBlock items='[{&quot;title&quot;:&quot;Confidence&quot;,&quot;description&quot;:&quot;95%&quot;}]' />
          </Card>
          <TextCallout variant="success" title="Backed by memory" content="Rendered at 2026-10-14" />
        </SectionBlock>
      `;
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0].type).toBe("SectionBlock");
			if (nodes?.[0].type === "SectionBlock") {
				expect(nodes[0].children).toHaveLength(3);
				expect(nodes[0].children[0].type).toBe("Card");
				expect(nodes[0].children[1].type).toBe("Card");
				expect(nodes[0].children[2].type).toBe("TextCallout");
			}
		});
	});

	describe("unknown tags degradation", () => {
		it("degrades unknown tags to text nodes", () => {
			const markup = "<CustomWidget>Some custom text payload</CustomWidget>";
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0]).toEqual(
				expect.objectContaining({
					type: "text",
					content: "Some custom text payload",
				}),
			);
		});

		it("degrades unknown tags inside known containers", () => {
			const markup =
				'<Card title="Mixed Card"><unknownElement>Degraded inside card</unknownElement></Card>';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			if (nodes?.[0].type === "Card") {
				expect(nodes[0].children[0]).toEqual(
					expect.objectContaining({
						type: "text",
						content: "Degraded inside card",
					}),
				);
			}
		});
	});

	describe("security and script injection stay inert", () => {
		it("degrades script tag to inert text node without executing", () => {
			const markup = '<script>alert("xss")</script>';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			expect(nodes?.[0]).toEqual(
				expect.objectContaining({
					type: "text",
					content: 'alert("xss")',
				}),
			);
		});

		it("safely handles elements with onerror or javascript href", () => {
			const markup = '<a href="javascript:alert(1)">Click me</a>';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			// Degrades to safe text node
			expect(nodes?.[0]).toEqual(
				expect.objectContaining({
					type: "text",
					content: "Click me",
				}),
			);
		});

		it("drops empty unknown tags like img with onerror", () => {
			const markup = '<img src="invalid.png" onerror="alert(1)" />';
			const nodes = parseOpenUi(markup);
			// Empty content inside unknown tag returns null
			expect(nodes).toBeNull();
		});
	});

	describe("malformed inputs return null", () => {
		it("returns null for null, undefined, and empty string", () => {
			expect(parseOpenUi(null)).toBeNull();
			expect(parseOpenUi(undefined)).toBeNull();
			expect(parseOpenUi("")).toBeNull();
			expect(parseOpenUi("   \n\t  ")).toBeNull();
		});

		it("returns null for plain text without any tags", () => {
			expect(
				parseOpenUi("Just a regular plain text message without tags."),
			).toBeNull();
		});

		it("returns null for unclosed XML tags", () => {
			expect(parseOpenUi("<SectionBlock><Card>unclosed")).toBeNull();
			expect(parseOpenUi('<Card title="foo')).toBeNull();
			expect(parseOpenUi('<TextContent content="abc"')).toBeNull();
		});

		it("returns null for mismatched XML tags", () => {
			expect(parseOpenUi('<Card title="Test"></SectionBlock>')).toBeNull();
		});

		it("returns null for syntax error in XML markup", () => {
			expect(parseOpenUi("<Card title= unquoted >text</Card>")).toBeNull();
		});
	});

	describe("non-string JSON item fields coercion", () => {
		it("coerces non-string JSON item fields in ListBlock without crashing", () => {
			const itemsJson = JSON.stringify([
				{
					title: { x: 1 },
					description: 12345,
					meta: true,
				},
				{
					title: 42,
					description: null,
					meta: undefined,
				},
			]);
			const markup = `<ListBlock items='${itemsJson.replace(/'/g, "&apos;")}' />`;
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			if (nodes?.[0].type === "ListBlock") {
				expect(nodes[0].items).toHaveLength(2);
				expect(nodes[0].items[0].title).toBe("[object Object]");
				expect(nodes[0].items[0].description).toBe("12345");
				expect(nodes[0].items[0].meta).toBe("true");
				expect(nodes[0].items[1].title).toBe("42");
				expect(nodes[0].items[1].description).toBeUndefined();
				expect(nodes[0].items[1].meta).toBeUndefined();
			}
		});

		it("coerces non-string JSON item fields in Accordion without crashing", () => {
			const itemsJson = JSON.stringify([
				{
					title: { nested: "object" },
					content: 999,
				},
				{
					title: 777,
					content: true,
				},
			]);
			const markup = `<Accordion items='${itemsJson.replace(/'/g, "&apos;")}' />`;
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			if (nodes?.[0].type === "Accordion") {
				expect(nodes[0].items).toHaveLength(2);
				expect(nodes[0].items[0].title).toBe("[object Object]");
				expect(nodes[0].items[0].content).toBe("999");
				expect(nodes[0].items[1].title).toBe("777");
				expect(nodes[0].items[1].content).toBe("true");
			}
		});
	});

	describe("empty and invisible nodes return null", () => {
		it("returns null for empty ListBlock items='[]'", () => {
			expect(parseOpenUi('<ListBlock items="[]" />')).toBeNull();
			expect(parseOpenUi('<ListBlock items="" />')).toBeNull();
			expect(parseOpenUi('<ListBlock items="   " />')).toBeNull();
		});

		it("returns null for empty Accordion items='[]'", () => {
			expect(parseOpenUi('<Accordion items="[]" />')).toBeNull();
			expect(parseOpenUi('<Accordion items="" />')).toBeNull();
			expect(parseOpenUi('<Accordion items="   " />')).toBeNull();
		});

		it("returns null for SectionBlock or Card containing only empty blocks", () => {
			expect(
				parseOpenUi("<SectionBlock><ListBlock items='[]' /></SectionBlock>"),
			).toBeNull();
			expect(
				parseOpenUi("<Card><Accordion items='[]' /></Card>"),
			).toBeNull();
		});
	});

	describe("unescaped '<' and '&' handling", () => {
		it("safely returns null when unescaped '<' causes XML syntax error", () => {
			expect(
				parseOpenUi(
					'<Card title="Score < 5"><TextContent content="A" /></Card>',
				),
			).toBeNull();
			expect(
				parseOpenUi(
					"<Card><TextContent>Unescaped < without closing</TextContent></Card>",
				),
			).toBeNull();
		});

		it("safely handles unescaped '&' in text and attributes by pre-normalizing", () => {
			const markup =
				'<Card title="Tom & Jerry"><TextContent content="Fish & Chips &amp; Soda" /></Card>';
			const nodes = parseOpenUi(markup);
			expect(nodes).not.toBeNull();
			if (nodes?.[0].type === "Card") {
				expect(nodes[0].title).toBe("Tom & Jerry");
				if (nodes[0].children[0].type === "TextContent") {
					expect(nodes[0].children[0].content).toBe("Fish & Chips & Soda");
				}
			}
		});

		it("returns null if text with unescaped '&' is also malformed XML", () => {
			expect(parseOpenUi("<Card>Unclosed tag & dangling")).toBeNull();
		});
	});
});
