#include <math.h>
#ifdef coshl
#undef coshl
#endif
long double (*foo)(long double) = coshl;
int main(void) { return 0; }
