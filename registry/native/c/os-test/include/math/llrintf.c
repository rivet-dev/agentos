#include <math.h>
#ifdef llrintf
#undef llrintf
#endif
long long (*foo)(float) = llrintf;
int main(void) { return 0; }
