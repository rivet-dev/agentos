#include <wchar.h>
#ifdef fgetwc
#undef fgetwc
#endif
wint_t (*foo)(FILE *) = fgetwc;
int main(void) { return 0; }
