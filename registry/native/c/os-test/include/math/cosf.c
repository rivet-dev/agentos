#include <math.h>
#ifdef cosf
#undef cosf
#endif
float (*foo)(float) = cosf;
int main(void) { return 0; }
