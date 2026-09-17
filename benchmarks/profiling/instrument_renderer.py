# Instrument a disposable copy only. Usage: python3 instrument_renderer.py SOURCE DEST
import sys
p=sys.argv[2]
s=open(sys.argv[1]).read()
s=s.replace('pub frames: u64,','pub frames: u64,\n pub clear_ns:u128, pub prepare_ns:u128,pub raster_ns:u128,pub sort_ns:u128,')
s=s.replace('let nodes = scene.ordered_nodes();','let start=std::time::Instant::now(); let nodes = scene.ordered_nodes();self.stats.sort_ns+=start.elapsed().as_nanos();')
s=s.replace('self.clear(area, scene.background);','let start=std::time::Instant::now(); self.clear(area, scene.background); self.stats.clear_ns+=start.elapsed().as_nanos();')
s=s.replace('let text = match &node.primitive {','let start=std::time::Instant::now(); let text = match &node.primitive {')
s=s.replace('self.paint_node(node, area, text.as_deref());','self.stats.prepare_ns+=start.elapsed().as_nanos();let start=std::time::Instant::now();self.paint_node(node, area, text.as_deref());self.stats.raster_ns+=start.elapsed().as_nanos();')
open(p,'w').write(s)
