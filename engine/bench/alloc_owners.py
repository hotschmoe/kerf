#!/usr/bin/env python3
"""alloc_owners.py <folded> [N] : share of samples inside libc (malloc/free/realloc/memcpy...) grouped by the innermost
engine frame that is not collection plumbing: who pays for allocation churn."""
import collections
import re
import sys

plumbing = re.compile(r'^(alloc::|core::|<alloc|<core|__rust|__rdl|std::|<std|grow|reserve|push|extend|clone|to_vec|to_owned|'
                      r'from_iter|collect|with_capacity|drop_in_place|spec_|finish_grow|allocate|alloc_impl|deallocate|'
                      r'copy_nonoverlapping|copy_from_slice|format|insert|append|to_string|from|into|new|default|next|fold|map)')
own, tot, libc = collections.Counter(), 0, 0
for line in open(sys.argv[1]):
    st, _, w = line.rstrip('\n').rpartition(' ')
    w = int(w)
    tot += w
    fr = st.split(';')
    if '[libc.so.6]' not in fr[-1]:
        continue
    libc += w
    owner = '?'
    for f in reversed(fr[:-1]):
        if '[libc.so.6]' in f or plumbing.match(f):
            continue
        owner = f
        break
    own[owner] += w
print(f'libc total {100 * libc / tot:.1f}% of samples')
for k, v in own.most_common(int(sys.argv[2]) if len(sys.argv) > 2 else 30):
    print(f'{100 * v / tot:5.2f}%  {k}')
