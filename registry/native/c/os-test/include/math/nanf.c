#include <math.h>
#ifdef nanf
#undef nanf
#endif
float (*foo)(const char *) = nanf;
int main(void) { return 0; }
