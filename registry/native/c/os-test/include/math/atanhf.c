#include <math.h>
#ifdef atanhf
#undef atanhf
#endif
float (*foo)(float) = atanhf;
int main(void) { return 0; }
