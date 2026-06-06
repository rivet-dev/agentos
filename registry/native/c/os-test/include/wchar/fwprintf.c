#include <wchar.h>
#ifdef fwprintf
#undef fwprintf
#endif
int (*foo)(FILE *restrict, const wchar_t *restrict, ...) = fwprintf;
int main(void) { return 0; }
