/*optional*/
#include <fenv.h>
#ifndef FE_INVALID
#error "FE_INVALID is not defined"
#endif
int main(void) { return 0; }
