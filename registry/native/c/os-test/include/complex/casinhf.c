#include <complex.h>
#ifdef casinhf
#undef casinhf
#endif
float complex (*foo)(float complex) = casinhf;
int main(void) { return 0; }
