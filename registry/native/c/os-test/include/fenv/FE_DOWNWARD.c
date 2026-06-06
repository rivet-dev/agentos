/*optional*/
#include <fenv.h>
#ifndef FE_DOWNWARD
#error "FE_DOWNWARD is not defined"
#endif
int main(void) { return 0; }
