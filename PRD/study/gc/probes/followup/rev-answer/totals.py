# Row totals for the revised ANSWER.md: itemize.md R1-R31, R26 widened, R32-R34 added.
a={1:(6,10),2:(3,5),3:(3,5),4:(2,4),5:(2,3),6:(2,4),7:(2,3),8:(1,2),9:(0,0),10:(2,4),11:(8,13),12:(2,4),13:(1,2),14:(2,3),
   15:(4,7),16:(4,8),17:(1,3),18:(3,6),19:(0,0),20:(2,3),21:(3,5),22:(1,2),23:(0.5,1),24:(1,2),25:(1,2),26:(4,7),27:(2,3),28:(2,3),29:(0.5,1),30:(6,10),31:(6,12)}
b={1:(0,0),2:(0,0),3:(1.5,3),4:(0.5,1),5:(1.5,2.5),6:(1,2),7:(1,2),8:(0.5,1),9:(1.5,3),10:(1,2),11:(2,4),12:(1,3),13:(1,2),14:(0.5,1),
   15:(2,3),16:(1.5,4),17:(1,2),18:(1.5,3),19:(0.5,1),20:(0.5,1),21:(1,2),22:(0.5,1),23:(0.5,0.5),24:(0.5,1),25:(0.5,1),26:(3,5),27:(1,2),28:(1,1.5),29:(0.5,1),30:(4,7),31:(3,6)}
S=lambda d,ks=None:(sum(d[k][0] for k in (ks or d)),sum(d[k][1] for k in (ks or d)))
print('original a',S(a),'b',S(b))
a[26]=(4,10); b[26]=(3,8)
for k,v in {32:(1,2),33:(1,2),34:(0.5,1.5)}.items(): a[k]=v; b[k]=v
A=S(a);B=S(b);print('revised a',A,'b',B,'months a',[round(x/4.33,1) for x in A],'b',[round(x/4.33,1) for x in B])
unpriced=[5,6,9,17,25,26,27,28,29,32,33,34]; delivered=[4,11,16,18,21]
U=S(b,unpriced);D=S(b,delivered);R=(B[0]-U[0]-D[0],B[1]-U[1]-D[1])
print('b unpriced-by-ST',U,'ST-delivered',D,'ST-comparable',R, S(b,[k for k in b if k not in unpriced+delivered]))
# from today: ST items mapped to (a) rows
st={'heap/arena 6-10':[1],'Rc->Arc 8-14':[2,3,6,7,8,10],'call sites 4-6':[11],'rooting 4-8':[16],'handshakes 3-5':[14,15],'code store 2-4':[4,22],'tests+recovery 8-16':[30,31],'obligations 2-4':[12,13]}
m=[k for v in st.values() for k in v]
for n,ks in st.items(): print(' ',n,'->',S(a,ks))
print('mapped',S(a,m),'unmapped',S(a,[k for k in a if k not in m]))
sav=(9,15); Bw=(B[0]-sav[0],B[1]-sav[1]); print('with additions',Bw,[round(x/4.33,1) for x in Bw])
for lab,(lo,hi) in [('build b',B),('build b+add',Bw),('build a',A)]:
    print(lab,'x2-4 years',round(lo*2/52,2),round(hi*4/52,2))
