#!/usr/bin/env python3
"""Require immutable dependency revisions for release validation and packaging."""
import os
import re

for name in ('ARCHIVE_RS', 'CABINET', 'MS_COMPRESS', 'WIM_RS', 'MKISO_RS'):
    value = os.environ.get(f'{name}_REF', '')
    if not re.fullmatch(r'[0-9a-fA-F]{40}', value):
        raise SystemExit(f'{name}_REF must be a full 40-character Git commit for a release')
print('All release dependency revisions are immutable commit IDs.')
