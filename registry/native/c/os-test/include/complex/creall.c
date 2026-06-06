#include <complex.h>
#ifdef creall
#undef creall
#endif
long double (*foo)(long double complex) = creall;
int main(void) { return 0; }
