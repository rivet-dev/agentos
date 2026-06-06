#include <fenv.h>
#ifndef FE_ALL_EXCEPT
#error "FE_ALL_EXCEPT is not defined"
#endif
int main(void) { return 0; }
