#include <math.h>
#ifdef erfl
#undef erfl
#endif
long double (*foo)(long double) = erfl;
int main(void) { return 0; }
