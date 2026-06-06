/*optional*/
#include <fenv.h>
#ifndef FE_UNDERFLOW
#error "FE_UNDERFLOW is not defined"
#endif
int main(void) { return 0; }
