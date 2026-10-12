import os, sys; sys.path.insert(0,os.path.join(os.path.dirname(os.path.abspath(__file__)),'..')); import emu_driver as D
from PIL import Image
S=os.environ.get('EMU_STATES', os.path.join(os.path.dirname(os.path.abspath(__file__)),'states'))  # save states + screenshots (ROM-derived, never commit)
os.makedirs(S,exist_ok=True)
def sheet(ims,cols,path):
    w,h=ims[0].size; rows=(len(ims)+cols-1)//cols; out=Image.new('RGB',(w*cols,h*rows))
    for i,im in enumerate(ims): out.paste(im,((i%cols)*w,(i//cols)*h))
    out.save(path)
