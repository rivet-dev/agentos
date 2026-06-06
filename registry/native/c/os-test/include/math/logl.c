#include <math.h>
#ifdef logl
#undef logl
#endif
long double (*foo)(long double) = logl;
int main(void) { return 0; }
