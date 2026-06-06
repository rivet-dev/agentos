#include <wchar.h>
#ifdef vfwscanf
#undef vfwscanf
#endif
int (*foo)(FILE *restrict, const wchar_t *restrict, va_list) = vfwscanf;
int main(void) { return 0; }
