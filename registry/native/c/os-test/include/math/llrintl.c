#include <math.h>
#ifdef llrintl
#undef llrintl
#endif
long long (*foo)(long double) = llrintl;
int main(void) { return 0; }
