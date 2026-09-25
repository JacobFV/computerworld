#!/usr/bin/env python3
"""Rewrites react-admin's simple example data (examples/simple/src/data.tsx) with the
world's own content before scripts/oss-web/build.sh builds it: the same records,
ids, dates, tags, relations and field types the demo's screens depend on, with the
lorem ipsum titles, teasers, bodies, commenters and users replaced by the posts of
a small engineering blog written by the people of worlds/oss-web.

    python3 scripts/oss-web/react-admin-data.py path/to/data.tsx

The file is edited in place; the script refuses a file whose shape it does not know.
"""
import re
import sys

POSTS = [
    ("How we cut CI time by caching the package store",
     "Restoring node_modules took longer than installing it. Caching the store instead made every build faster.",
     ["We cached <strong>node_modules</strong> between CI runs for two years. Restoring it took three minutes; a clean install from a warm store took forty seconds.",
      "The store is content-addressed and small. Cache the store, not the tree it produces."]),
    ("Retries are a decision not to find out",
     "Every retry hides a failure. Keep the ones you mean, and log them loudly.",
     ["Our payment service retried every failed call five times. The retries masked a race in the connection pool.",
      "We removed all but one, and that one is logged every time it fires."]),
    ("Native form validation, revisited",
     "Required fields, patterns and lengths are already in the browser, and so are the error messages.",
     ["We moved three forms back to constraint validation and <em>FormData</em>. The bundle lost 38 KB.",
      "The forms also got more accessible, because the browser announces its own errors."]),
    ("Partitioning the jobs table",
     "The weekly export queue backed up every Tuesday. Dropping old partitions fixed it.",
     ["Autovacuum could not keep up with the dead tuples a weekly job left behind.",
      "Partitioning by week turned deletes into dropped tables, and the backlog went away."]),
    ("Eight spacing tokens are enough",
     "We cut our spacing scale from twenty-three values to eight, named by role.",
     ["Inset, stack and inline: naming tokens by what they do ended most spacing debates in review."]),
    ("An on-call rotation people volunteer for",
     "Actionable pages, written handoffs and a day off after a bad night.",
     ["We deleted sixty percent of our alerts. Every remaining page links a runbook and names an owner.",
      "Handoffs are a written note, and the day after a bad night is a day off, taken."]),
    ("Passkeys six months in",
     "Forty-one percent of weekly sign-ins now use a passkey. Account recovery was the hard part.",
     ["Support tickets about locked accounts fell by half once passkeys shipped.",
      "Every recovery path had to be as strong as the passkey it replaced."]),
    ("Reproducible builds, byte for byte",
     "Pinning the toolchain was not enough: timestamps, file order and a locale-dependent sort all leaked in.",
     ["We diffed two builds of the same commit, byte by byte, and fixed each difference in turn."]),
    ("Idempotency keys for payments",
     "Charge the card once however many times the network makes the client ask.",
     ["The server stores the key with the first result and returns that result to every retry.",
      "Keys expire after a day, which covers every client we have."]),
    ("Data contracts between teams",
     "A published schema for every shared table, checked in CI.",
     ["Most data incidents were a producer changing a column a consumer relied on. Contracts catch that before the dashboard does."]),
    ("Choosing a queue for background work",
     "Postgres, Redis or a managed queue: what we weighed and what we picked.",
     ["We already ran Postgres well, and our volume fit comfortably in a table with SKIP LOCKED.",
      "We will revisit when a single worker pool is no longer enough."]),
    ("Flaky tests are a budget",
     "We priced every flaky test in reruns and fixed the expensive ones first.",
     ["A flaky test costs a rerun of its whole job. Ranking them by cost, not by count, changed which ones we fixed."]),
    ("Writing incident reviews people read",
     "A timeline, what surprised us, and what we will change: three sections and nothing else.",
     ["Our reviews used to be twelve pages. Now they are one, and people read them.",
      "The section that matters most is what surprised us."]),
]

# In the order of the demo's comments, which belong to posts 6, 9, 3, 6, 1, 6, 5, 5, 2, 3, 1.
COMMENTS = [
    ("Kenji Watanabe", "kenji@watanabe.jp", "The day off after a bad night should be policy everywhere."),
    ("Wren Castillo", "wren@castillo.io", "We keep keys for a day too; it covers every mobile client we have."),
    ("Jules Moreau", "jules@moreau.fr", "The accessibility point is underrated."),
    ("Priya Natarajan", "priya@natarajan.dev", "Deleting alerts was the hardest part for us, and the most useful."),
    ("Tomas Reyes", "tomas@reyes.dev", "Which package manager? We saw this with pnpm but not with npm."),
    ("Fatima Haddad", "fatima@haddad.dev", "Written handoffs saved us during the last incident."),
    ("Ada Okonkwo", "ada@okonkwo.dev", "Naming tokens by role is the part we will steal."),
    ("Tomas Reyes", "tomas@reyes.dev", "Eight feels like a lot until you try six."),
    ("Kenji Watanabe", "kenji@watanabe.jp", "Keeping one retry and logging it is the part people skip."),
    ("Wren Castillo", "wren@castillo.io", "38 KB is a lot of validation code to have been carrying."),
    ("Ada Okonkwo", "ada@okonkwo.dev", "Caching the store works with npm too: it is ~/.npm."),
]

USERS = {"Logan Schowalter": "Wren Castillo", "Breanna Gibson": "Ada Okonkwo", "Annamarie Mayer": "Tomas Reyes"}
COMMENTERS = ["Kiley Pouros", "Justina Hegmann", "Ms. Brionna Smitham MD", "Edmond Schulist",
              "Danny Greenholt", "Luciano Berge"]


def js(s):
    return "'" + s.replace("\\", "\\\\").replace("'", "\\'") + "'"


def main():
    path = sys.argv[1]
    s = open(path, encoding="utf-8").read()
    posts, rest = s.split("\n    comments: [", 1)
    comments, rest = rest.split("\n    tags: [", 1)
    titles = re.findall(r"\n            title: (?:'(?:[^'\\]|\\.)*'|\"[^\"]*\"),", posts)
    teasers = re.findall(r"\n            teaser:\s*(?:'(?:[^'\\]|\\.)*'|\"[^\"]*\"),", posts)
    bodies = re.findall(r"\n            body:\s*(?:'(?:[^'\\]|\\.)*'|\"[^\"]*\"),", posts)
    if not (len(titles) == len(teasers) == len(bodies) == len(POSTS)):
        sys.exit(f"{path}: expected {len(POSTS)} posts, found {len(titles)} titles, {len(teasers)} teasers, {len(bodies)} bodies")
    for old, (title, _, _) in zip(titles, POSTS):
        posts = posts.replace(old, f"\n            title: {js(title)},", 1)
    for old, (_, teaser, _) in zip(teasers, POSTS):
        posts = posts.replace(old, f"\n            teaser: {js(teaser)},", 1)
    for old, (_, _, body) in zip(bodies, POSTS):
        html = "".join(f"<p>{p}</p>" for p in body)
        posts = posts.replace(old, f"\n            body: {js(html)},", 1)
    # The picture metadata of the first post names people too.
    posts = posts.replace("'Paul'", "'Ada Okonkwo'").replace("'paul@email.com'", "'ada@okonkwo.dev'")
    posts = posts.replace("'Joe'", "'Wren Castillo'").replace("'joe@email.com'", "'wren@castillo.io'")
    cbodies = re.findall(r"\n            body:\s*(?:'(?:[^'\\]|\\.)*'|\"(?:[^\"\\]|\\.)*\"),", comments)
    authors = re.findall(r"\n            author: \{[^}]*\},", comments)
    if len(cbodies) != len(COMMENTS) or len(authors) != len(COMMENTS):
        sys.exit(f"{path}: expected {len(COMMENTS)} comments, found {len(cbodies)} bodies and {len(authors)} authors")
    for old, (_, _, body) in zip(cbodies, COMMENTS):
        comments = comments.replace(old, f"\n            body: {js(body)},", 1)
    for old, (name, email, _) in zip(authors, COMMENTS):
        new = ("\n            author: {\n                name: " + js(name) + ",\n                email: "
               + js(email) + ",\n            },")
        comments = comments.replace(old, new, 1)
    for old in COMMENTERS:
        comments = comments.replace(js(old), js("Wren Castillo"))
    for old, new in USERS.items():
        rest = rest.replace(js(old), js(new))
        posts = posts.replace(js(old), js(new))
        comments = comments.replace(js(old), js(new))
    out = posts + "\n    comments: [" + comments + "\n    tags: [" + rest
    # The demo's dates are 2012; the world's blog ran through late 2025.
    out = out.replace("'2012-", "'2025-")
    out = out.replace("https://foo.bar.com/lorem/ipsum", "http://conduit.realworld.show/")
    out = out.replace("http://dicta.es/nam_doloremque", "http://news.ycombinator.com/")
    if re.search(r"Accusantium|Fusce massa|Pouros|Hegmann|<p>[A-Z][a-z]+ [a-z]+ [a-z]+ (?:et|aut|est)\b", out):
        sys.exit(f"{path}: placeholder text is left")
    open(path, "w", encoding="utf-8").write(out)
    print(f"rewrote {path}: {len(POSTS)} posts, {len(COMMENTS)} comments")


if __name__ == "__main__":
    main()
