#include <wctype.h>
#ifdef towupper
#undef towupper
#endif
wint_t (*foo)(wint_t) = towupper;
int main(void) { return 0; }
