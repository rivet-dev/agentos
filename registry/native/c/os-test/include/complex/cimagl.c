#include <complex.h>
#ifdef cimagl
#undef cimagl
#endif
long double (*foo)(long double complex) = cimagl;
int main(void) { return 0; }
