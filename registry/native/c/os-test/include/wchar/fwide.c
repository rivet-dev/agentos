#include <wchar.h>
#ifdef fwide
#undef fwide
#endif
int (*foo)(FILE *, int) = fwide;
int main(void) { return 0; }
