/*optional*/
#include <fenv.h>
#ifndef FE_DIVBYZERO
#error "FE_DIVBYZERO is not defined"
#endif
int main(void) { return 0; }
