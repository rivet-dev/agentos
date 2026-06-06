#include <math.h>
#ifdef llroundl
#undef llroundl
#endif
long long (*foo)(long double) = llroundl;
int main(void) { return 0; }
