// Experimental measurement of the supplied client SpeedTreeRT leaf rocking.
#define wmain old_bake_main
#include "bake_experimental.cpp"
#undef wmain
using Clock=void(*)(float);
using Strength=float(METHOD*)(void*,float,float,float);
using Rock=void(METHOD*)(void*,bool);
using Groups=void(METHOD*)(void*,unsigned);
int wmain(int argc,wchar_t**argv){
 if(argc!=3)return 2;
 Api a=open_api(argv[1]);
 auto clock=GET(Clock,"?SetTime@CSpeedTreeRT@@SAXM@Z");
 auto strength=GET(Strength,"?SetWindStrength@CSpeedTreeRT@@QAEMMMM@Z");
 auto rock=GET(Rock,"?SetLeafRockingState@CSpeedTreeRT@@QAEX_N@Z");
 auto groups=GET(Groups,"?SetNumLeafRockingGroups@CSpeedTreeRT@@QAEXI@Z");
 FILE*f=_wfopen(argv[2],L"rb");if(!f)return 3;fseek(f,0,SEEK_END);long n=ftell(f);rewind(f);std::vector<uint8_t> data(n);fread(data.data(),1,n,f);fclose(f);
 void* inst=a.alloc(0x9c);a.ctor(inst);if(!a.load(inst,data.data(),n))return 4;
 float size,var;a.getsize(inst,&size,&var);a.setbranch(inst,0);a.setleaf(inst,0);a.setfrond(inst,0);a.windb(inst,2);a.windl(inst,2);a.windf(inst,2);groups(inst,1);a.setsize(inst,size*0.3048f,0);if(!a.compute(inst,nullptr,1,true))return 5;rock(inst,true);Geometry g{};wanted_lod=0.5f;
 rock(inst,false);clock(0);strength(inst,0.f,-1.f,-1.f);auto staticbase=table_at(a,inst,g,1,0,0);auto up=table_at(a,inst,g,0,0,1);auto down=table_at(a,inst,g,0,0,-1);std::vector<float> rhos(up.size()/16);for(unsigned k=0;k<rhos.size();k++){float x=0,y=0,best=0;for(int j=0;j<4;j++){float ax=(up[k*16+j*4]+down[k*16+j*4])*0.5f,ay=(up[k*16+j*4+1]+down[k*16+j*4+1])*0.5f;float len=ax*ax+ay*ay;if(len>best){best=len;x=ax;y=ay;}}if(y<0){x=-x;y=-y;}rhos[k]=best>1e-6f?atan2f(y,x)-1.57079632679f:0.f;}rock(inst,true);clock(0);strength(inst,1.f,-1.f,-1.f);auto base=table_at(a,inst,g,1,0,0);printf("table entries %u leaves %u branch weights %p indices %p leaf weights %p indices %p\n",(unsigned)base.size(),g.leaves[0].count,g.branches.weights,g.branches.wind,g.leaves[0].weights,g.leaves[0].wind);
 for(float t: {0.f,0.25f,0.5f,1.f,2.f,4.f,8.f,16.f}){clock(t);strength(inst,1.f,-1.f,-1.f);auto cur=table_at(a,inst,g,1,0,0);float card_error=0;unsigned card_count=0;if(g.leaves[0].mapcoords && g.leaves[0].clusters){for(unsigned i=0;i<g.leaves[0].count;i++){auto pp=g.leaves[0].mapcoords[i];unsigned c=g.leaves[0].clusters[i];if(!pp || c*16+15>=cur.size())continue;for(int j=0;j<4;j++)for(int k=0;k<3;k++){card_error=std::max(card_error,fabsf(pp[j*4+k]-cur[c*16+j*4+k]));card_count++;}}}float within=0;const float* groups[256]={};if(g.leaves[0].mapcoords&&g.leaves[0].clusters)for(unsigned i=0;i<g.leaves[0].count;i++){unsigned c=g.leaves[0].clusters[i];auto pp=g.leaves[0].mapcoords[i];if(!pp)continue;if(!groups[c])groups[c]=pp;for(int k=0;k<16;k++)within=std::max(within,fabsf(pp[k]-groups[c][k]));}printf("per-leaf corner vs cluster table max %.6f same-cluster max %.6f values %u\n",card_error,within,card_count);float mx=0,avg=0;for(unsigned i=0;i<cur.size();i++){float d=fabsf(cur[i]-base[i]);mx=std::max(mx,d);avg+=d;}auto other=table_at(a,inst,g,0.5f,0.5f,0.70710678f);float err=0;for(unsigned k=0;k<rhos.size();k++){float r=rhos[k],c=cosf(r),ss=sinf(r),ca=cosf(0.78539816339f+r),sa=sinf(0.78539816339f+r),ce=0.70710678f,se=ce;for(int j=0;j<4;j++){unsigned p=k*16+j*4;float x=c*cur[p]+ss*cur[p+1],y=-ss*cur[p]+c*cur[p+1],z=cur[p+2],qx=ce*x-se*z,qz=se*x+ce*z;float pred[3]={ca*qx-sa*y,sa*qx+ca*y,qz};for(int c2=0;c2<3;c2++)err=std::max(err,fabsf(pred[c2]-other[p+c2]));}}printf("time %.2f max-delta %.6f avg %.6f camera-model-error %.6f first %.6f\n",t,mx,avg/cur.size(),err,cur[0]);}
 a.dtor(inst);a.free(inst);FreeLibrary(a.dll);return 0;
}
