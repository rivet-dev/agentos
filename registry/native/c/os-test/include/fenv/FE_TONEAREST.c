/*optional*/
#include <fenv.h>
#ifndef FE_TONEAREST
#error "FE_TONEAREST is not defined"
#endif
int main(void) { return 0; }
