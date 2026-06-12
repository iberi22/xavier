import re

with open('benches/embedding_benchmark.rs', 'rb') as f:
    content = f.read()

# Find LOW similarity pairs block
target = b'LOW similarity pairs (should return < 0.4)'
idx = content.index(target)
end = content.index(b']', idx)
pair_start = content.index(b'SimilarityPair', idx)

new_pairs = b'SimilarityPair {\n'
new_pairs += b'            query: "best pasta carbonara recipe with guanciale and pecorino",\n'
new_pairs += b'            doc: "The Pythagoreans believed numbers were the fundamental essence of all reality.",\n'
new_pairs += b'            expected_high: false,\n'
new_pairs += b'        },\n'
new_pairs += b'        SimilarityPair {\n'
new_pairs += b'            query: "how to fix leaking kitchen sink pipe under cabinet",\n'
new_pairs += b'            doc: "Coral reefs are diverse underwater ecosystems held together by calcium carbonate.",\n'
new_pairs += b'            expected_high: false,\n'
new_pairs += b'        },\n'
new_pairs += b'        SimilarityPair {\n'
new_pairs += b'            query: "mercedes-benz e-class 2025 fuel efficiency warranty",\n'
new_pairs += b'            doc: "Quantum entanglement occurs when particles become interconnected instantly.",\n'
new_pairs += b'            expected_high: false,\n'
new_pairs += b'        },\n'
new_pairs += b'        SimilarityPair {\n'
new_pairs += b'            query: "yoga poses for lower back pain relief beginners",\n'
new_pairs += b'            doc: "The Rosetta Stone was key to deciphering Egyptian hieroglyphs through its scripts.",\n'
new_pairs += b'            expected_high: false,\n'
new_pairs += b'        }'

new_content = content[:pair_start] + new_pairs + content[end:]
with open('benches/embedding_benchmark.rs', 'wb') as f:
    f.write(new_content)

print('Done - replaced LOW pairs with properly unrelated topics')
