/*optional*/
#include <fenv.h>
#ifndef FE_INEXACT
#error "FE_INEXACT is not defined"
#endif
int main(void) { return 0; }
