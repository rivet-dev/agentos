/*optional*/
#include <fenv.h>
#ifndef FE_OVERFLOW
#error "FE_OVERFLOW is not defined"
#endif
int main(void) { return 0; }
