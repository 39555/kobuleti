import re
import sys


file = sys.argv[1]
print(file)
p = re.compile(r'\x1b\[(\d+)C')
with open("a.txt", 'w') as new:
    with open(file, 'r') as f:
        for l in f:
            for r in re.finditer(p, l):
                l = l.replace(r.group(0), ' ' * int(r.group(1)))
            new.write(l)


        




