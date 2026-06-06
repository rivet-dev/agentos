/*optional*/
#include <fenv.h>
#ifndef FE_TOWARDZERO
#error "FE_TOWARDZERO is not defined"
#endif
int main(void) { return 0; }
