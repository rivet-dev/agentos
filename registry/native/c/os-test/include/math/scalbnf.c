#include <math.h>
#ifdef scalbnf
#undef scalbnf
#endif
float (*foo)(float, int) = scalbnf;
int main(void) { return 0; }
