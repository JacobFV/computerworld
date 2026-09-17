# Create a world

Start with [`examples/custom-world`](../examples/custom-world), or copy the
structure of [`worlds/company-2026/world.json`](../worlds/company-2026/world.json)
and replace its fixtures. The kernel has no required company domain or machine.

1. Define OS profiles and each machine's address, user and initial files.
2. Define service nodes and explicit links from clients to those nodes.
3. Place service instances with registered kinds and state. Give them domains.
4. Declare DNS records and gateway restrictions as needed.
5. Parse using `WorldDefinition::from_json`, construct `World`, and create an
   environment granting only the intended machines and action families.
6. Test behavior from actor actions: resolving, navigating, mutating and reading
   from a second machine. Owner inspection is diagnostic evidence, not the actor's solution.

Keep secret evaluation answers outside the blueprint's actor-visible files and
service responses. Use deterministic context values for generated fixtures.
Store the definition, engine version, module versions, seed and action sequence
when sharing a reproducible episode.
