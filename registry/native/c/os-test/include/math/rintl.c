#include <math.h>
#ifdef rintl
#undef rintl
#endif
long double (*foo)(long double) = rintl;
int main(void) { return 0; }
