import json
from pathlib import Path

import torch
import flash_attn2

meta_path = Path(flash_attn2.__file__).parent.parent / "metadata.json"
metadata = json.loads(meta_path.read_text())

print(f"kernel: {metadata['name']} v{metadata['version']}")
print(f"backend: {metadata['backend']['type']} archs={metadata['backend'].get('archs', [])}")
print(f"torch: {torch.__version__}")

if not torch.cuda.is_available():
    print("cuda: unavailable; import smoke-test only")
    raise SystemExit

print(f"cuda: {torch.cuda.get_device_name(0)} sm_{torch.cuda.get_device_capability(0)[0]}{torch.cuda.get_device_capability(0)[1]}")
q = k = v = torch.randn(1, 16, 2, 64, device="cuda", dtype=torch.float16)
out = flash_attn2.flash_attn_func(q, k, v)
print(f"flash attention output: shape={tuple(out.shape)} dtype={out.dtype}")
