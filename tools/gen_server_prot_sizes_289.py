import json, pathlib
p=json.load(open("docs/revision-289/protocol-289.json"))
s=[None]*256
for row in p["inbound"]:
    s[row["id"]]=row["length"]
assert all(x is not None for x in s)
lines=["pub const SERVER_PROT_SIZES_289: [i32; 256] = ["]
lines += [f"    {v}," for v in s]
lines.append("];")
pathlib.Path("crates/client/src/io/server_prot_sizes_289.inc.rs").write_text("\n".join(lines)+"\n")
print(s[76], s[107], s[121], s[47], len(s))
