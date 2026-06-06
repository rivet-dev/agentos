#include <math.h>
#ifdef acosh
#undef acosh
#endif
double (*foo)(double) = acosh;
int main(void) { return 0; }
