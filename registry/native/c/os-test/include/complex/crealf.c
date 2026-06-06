#include <complex.h>
#ifdef crealf
#undef crealf
#endif
float (*foo)(float complex) = crealf;
int main(void) { return 0; }
